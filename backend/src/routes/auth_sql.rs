use std::sync::Arc;

use axum::{
    extract::{FromRef, Query, State},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    db::{minio::MinioClient, sql_store::{
        DynSqlStore, NewUser, RotateCredentialsUpdate, RotateVaultEntry,
        StoredMindMap, UserProfileUpdate,
    }},
    error::AppError,
    middleware::auth::{AuthenticatedUser, JwtService},
    models::{
        access::{AccessPlan, AccessSource, SubscriptionMode, UiSurface, UserAccessGrant},
        attachment::AttachmentStatus,
        mindmap::VersionSnapshot,
        settings::{UpdateUserAccountSettingsRequest, UserAccountSettings},
        user::{
            AccountCapabilitiesResponse, AccountStorageResponse, KeyBundleResponse,
            LoginRequest, LoginResponse, ProfileResponse, RegisterRequest,
            RotateCredentialsRequest, SaltResponse, SubscriptionSummaryResponse,
            SubscriptionTier, UpdateProfileRequest,
        },
    },
};

#[derive(Clone)]
pub struct AuthSqlState {
    pub db: DynSqlStore,
    pub minio: MinioClient,
    pub jwt: Arc<JwtService>,
}

impl FromRef<AuthSqlState> for Arc<JwtService> {
    fn from_ref(state: &AuthSqlState) -> Self {
        state.jwt.clone()
    }
}

pub fn router(state: AuthSqlState) -> Router {
    Router::new()
        .route("/salt", get(get_salt))
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/keys", get(get_keys))
        .route("/rotate-credentials", post(rotate_credentials))
        .route("/subscription", get(get_subscription))
        .route("/capabilities", get(get_capabilities))
        .route("/storage", get(get_storage))
        .route("/settings", get(get_settings).patch(update_settings))
        .route("/profile", get(get_profile).put(update_profile).delete(delete_profile))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct SaltQuery {
    username: String,
}

#[derive(Debug, Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

async fn get_salt(
    State(state): State<AuthSqlState>,
    Query(q): Query<SaltQuery>,
) -> Result<Json<SaltResponse>, AppError> {
    if q.username.is_empty() {
        return Err(AppError::BadRequest("username is required".to_string()));
    }

    let user = state
        .db
        .load_user_by_username(&q.username)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    Ok(Json(SaltResponse {
        argon2_salt: user.argon2_salt,
        argon2_params: user.argon2_params,
    }))
}

async fn register(
    State(state): State<AuthSqlState>,
    Json(body): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.username.trim().is_empty() {
        return Err(AppError::BadRequest("username cannot be empty".to_string()));
    }
    if body.username.len() > 64 {
        return Err(AppError::BadRequest("username too long".to_string()));
    }
    if body.auth_token.is_empty() || body.argon2_salt.is_empty() {
        return Err(AppError::BadRequest("missing required crypto fields".to_string()));
    }

    if state.db.load_user_by_username(&body.username).await?.is_some() {
        return Err(AppError::Conflict("username already taken".to_string()));
    }

    let user_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let auth_hash = hash_auth_token(&body.auth_token)?;

    state
        .db
        .create_user(NewUser {
            id: user_id,
            username: body.username.trim().to_string(),
            auth_hash,
            argon2_salt: body.argon2_salt,
            argon2_params: body.argon2_params,
            classical_public_key: body.classical_public_key,
            pq_public_key: body.pq_public_key,
            classical_priv_encrypted: body.classical_priv_encrypted,
            pq_priv_encrypted: body.pq_priv_encrypted,
            key_version: 1,
            created_at: now,
            subscription_tier: SubscriptionTier::Free,
            stripe_customer_id: None,
            stripe_subscription_id: None,
            stripe_subscription_status: None,
            subscription_current_period_end: None,
            first_name: None,
            last_name: None,
            email: None,
            is_locked: false,
            locked_reason: None,
            admin_note: None,
            manual_subscription_tier: None,
            manual_subscription_expires_at: None,
            manual_subscription_reason: None,
            manual_subscription_granted_by: None,
            access_grants: vec![UserAccessGrant {
                subscription_mode: SubscriptionMode::PrivateEncrypted,
                ui_surface: UiSurface::EncryptedVaultApp,
                plan: AccessPlan::Free,
                source: AccessSource::LegacyBase,
                granted_at: now,
                expires_at: None,
                note: Some("Default encrypted app access".to_string()),
            }],
        })
        .await?;

    tracing::info!("Registered new user: {}", body.username);
    Ok(Json(serde_json::json!({ "message": "registered successfully" })))
}

async fn login(
    State(state): State<AuthSqlState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    if body.username.is_empty() || body.auth_token.is_empty() {
        return Err(AppError::BadRequest("username and auth_token are required".to_string()));
    }

    let user = state
        .db
        .load_user_by_username(&body.username)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid credentials".to_string()))?;

    if user.is_locked {
        return Err(AppError::Unauthorized("account is locked".to_string()));
    }

    verify_auth_token(&body.auth_token, &user.auth_hash)
        .map_err(|_| AppError::Unauthorized("invalid credentials".to_string()))?;

    let access_token = state.jwt.issue_access_token(&user.id)?;
    let refresh_token = state.jwt.issue_refresh_token(&user.id)?;

    tracing::info!("User logged in: {}", body.username);

    Ok(Json(LoginResponse {
        access_token,
        refresh_token,
        classical_public_key: user.classical_public_key,
        pq_public_key: user.pq_public_key,
        classical_priv_encrypted: user.classical_priv_encrypted,
        pq_priv_encrypted: user.pq_priv_encrypted,
        argon2_salt: user.argon2_salt,
        argon2_params: user.argon2_params,
        key_version: user.key_version,
    }))
}

async fn refresh(
    State(state): State<AuthSqlState>,
    Json(body): Json<RefreshRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let claims = state.jwt.validate_refresh_token(&body.refresh_token)?;
    let user = state
        .db
        .load_user_by_id(&claims.sub)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?;
    if user.is_locked {
        return Err(AppError::Unauthorized("account is locked".to_string()));
    }
    let access_token = state.jwt.issue_access_token(&claims.sub)?;
    Ok(Json(serde_json::json!({ "access_token": access_token })))
}

async fn get_keys(
    State(state): State<AuthSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<KeyBundleResponse>, AppError> {
    let db_user = state
        .db
        .load_user_by_id(&user.0)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    Ok(Json(KeyBundleResponse {
        classical_public_key: db_user.classical_public_key,
        pq_public_key: db_user.pq_public_key,
        classical_priv_encrypted: db_user.classical_priv_encrypted,
        pq_priv_encrypted: db_user.pq_priv_encrypted,
        argon2_salt: db_user.argon2_salt,
        argon2_params: db_user.argon2_params,
        key_version: db_user.key_version,
    }))
}

async fn get_profile(
    State(state): State<AuthSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<ProfileResponse>, AppError> {
    let db_user = state
        .db
        .load_user_by_id(&user.0)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    let effective_tier = db_user.effective_subscription_tier(Utc::now());
    let access_grants = db_user.effective_access_grants(Utc::now());

    Ok(Json(ProfileResponse {
        username: db_user.username,
        first_name: db_user.first_name,
        last_name: db_user.last_name,
        email: db_user.email,
        subscription_tier: effective_tier.as_str().to_string(),
        storage_limit_bytes: effective_tier.storage_limit_bytes(),
        subscription_current_period_end: db_user.subscription_current_period_end,
        access_grants,
    }))
}

async fn get_subscription(
    State(state): State<AuthSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<SubscriptionSummaryResponse>, AppError> {
    let db_user = state
        .db
        .load_user_by_id(&user.0)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    let now = Utc::now();
    let effective_tier = db_user.effective_subscription_tier(now);
    let plan_source = db_user.effective_plan_source(now).to_string();
    let stripe_customer_id_present = db_user.stripe_customer_id.is_some();
    let stripe_subscription_status = db_user.stripe_subscription_status.clone();
    let subscription_current_period_end = db_user.subscription_current_period_end;
    let manual_override_active = db_user.manual_subscription_active(now);

    Ok(Json(SubscriptionSummaryResponse {
        subscription_tier: effective_tier.as_str().to_string(),
        plan_source,
        storage_limit_bytes: effective_tier.storage_limit_bytes(),
        stripe_customer_id_present,
        stripe_subscription_status,
        subscription_current_period_end,
        manual_override_active,
    }))
}

async fn get_storage(
    State(state): State<AuthSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<AccountStorageResponse>, AppError> {
    let db_user = state
        .db
        .load_user_by_id(&user.0)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;
    let subscription_tier = db_user.effective_subscription_tier(Utc::now());

    let maps = state.db.list_mind_maps(&user.0).await?;
    let mut total_bytes = 0_i64;
    let mut attachment_count = 0_usize;
    let mut attachment_bytes = 0_i64;

    for map in &maps {
        let expected_version_ids = known_version_ids(&map.version_history, map.minio_version_id.as_deref());
        let versions = state
            .minio
            .merge_known_versions(
                &map.minio_object_key,
                &expected_version_ids,
                map.minio_version_id.as_deref(),
            )
            .await?;
        total_bytes += versions.iter().map(|version| version.size_bytes).sum::<i64>();
        let attachments = state.db.list_mind_map_attachments(&map.id).await?;
        attachment_count += attachments
            .iter()
            .filter(|attachment| attachment.status == AttachmentStatus::Available)
            .count();
        let map_attachment_bytes = attachments
            .iter()
            .filter(|attachment| attachment.status == AttachmentStatus::Available)
            .map(|attachment| attachment.size_bytes)
            .sum::<i64>();
        attachment_bytes += map_attachment_bytes;
        total_bytes += map_attachment_bytes;
        total_bytes += load_map_share_storage_bytes(&state.db, &map.id).await?;
    }

    let plan_limit_bytes = subscription_tier.storage_limit_bytes();
    Ok(Json(AccountStorageResponse {
        total_bytes,
        attachment_count,
        attachment_bytes,
        plan_tier: subscription_tier.as_str().to_string(),
        plan_limit_bytes,
        remaining_bytes: (plan_limit_bytes - total_bytes).max(0),
        over_limit: total_bytes > plan_limit_bytes,
        vault_count: maps.len(),
    }))
}

async fn get_capabilities(
    State(state): State<AuthSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<AccountCapabilitiesResponse>, AppError> {
    let db_user = state
        .db
        .load_user_by_id(&user.0)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;
    let subscription_tier = db_user.effective_subscription_tier(Utc::now());

    Ok(Json(AccountCapabilitiesResponse {
        plan_tier: subscription_tier.as_str().to_string(),
        storage_limit_bytes: subscription_tier.storage_limit_bytes(),
        max_attachment_size_bytes: subscription_tier.max_attachment_size_bytes(),
        max_active_shares: subscription_tier.max_active_shares(),
        can_create_public_shares: subscription_tier.can_create_public_shares(),
        can_include_attachments_in_shares: subscription_tier.can_include_attachments_in_shares(),
        can_use_plaintext_collaboration: subscription_tier.can_use_plaintext_collaboration(),
        can_export_large_maps: subscription_tier.can_export_large_maps(),
        can_use_admin_controls: subscription_tier.can_use_admin_controls(),
    }))
}

async fn get_settings(
    State(state): State<AuthSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<UserAccountSettings>, AppError> {
    let settings = state
        .db
        .load_user_account_settings(&user.0)
        .await?
        .unwrap_or_default();

    Ok(Json(settings))
}

async fn update_settings(
    State(state): State<AuthSqlState>,
    user: AuthenticatedUser,
    Json(body): Json<UpdateUserAccountSettingsRequest>,
) -> Result<Json<UserAccountSettings>, AppError> {
    let mut settings = state
        .db
        .load_user_account_settings(&user.0)
        .await?
        .unwrap_or_default();

    if let Some(value) = body.locale {
        settings.locale = normalize_bounded_string(value, "locale", 16)?;
    }
    if let Some(value) = body.timezone {
        settings.timezone = normalize_bounded_string(value, "timezone", 64)?;
    }
    if let Some(value) = body.date_format {
        settings.date_format = normalize_choice(value, "date_format", &["iso", "us", "eu"])?;
    }
    if let Some(value) = body.accessibility_reduce_motion {
        settings.accessibility_reduce_motion = value;
    }
    if let Some(value) = body.sync_appearance_across_devices {
        settings.sync_appearance_across_devices = value;
    }
    if let Some(value) = body.default_share_expiry_days {
        if !(1..=365).contains(&value) {
            return Err(AppError::BadRequest(
                "default_share_expiry_days must be between 1 and 365".to_string(),
            ));
        }
        settings.default_share_expiry_days = value;
    }
    if let Some(value) = body.default_include_attachments_on_share {
        settings.default_include_attachments_on_share = value;
    }
    if let Some(value) = body.default_map_layout {
        settings.default_map_layout = normalize_choice(
            value,
            "default_map_layout",
            &["mindmap", "tree", "outline", "kanban"],
        )?;
    }
    if let Some(value) = body.default_map_theme {
        settings.default_map_theme = normalize_choice(
            value,
            "default_map_theme",
            &["system", "light", "dark", "focus"],
        )?;
    }
    if let Some(value) = body.default_export_format {
        settings.default_export_format = normalize_choice(
            value,
            "default_export_format",
            &["cryptmind", "json", "markdown", "png"],
        )?;
    }
    if let Some(value) = body.default_node_style_preset {
        settings.default_node_style_preset = normalize_bounded_string(
            value,
            "default_node_style_preset",
            32,
        )?;
    }
    if let Some(value) = body.user_labels_json {
        // Validate it is parseable JSON array; store as-is up to 64 KB.
        let trimmed = value.trim().to_string();
        if trimmed.len() > 65536 {
            return Err(AppError::BadRequest("user_labels_json too large".to_string()));
        }
        let parsed: serde_json::Value = serde_json::from_str(&trimmed)
            .map_err(|_| AppError::BadRequest("user_labels_json must be valid JSON".to_string()))?;
        if !parsed.is_array() {
            return Err(AppError::BadRequest(
                "user_labels_json must be a JSON array".to_string(),
            ));
        }
        settings.user_labels_json = trimmed;
    }
    settings.updated_at = Utc::now();

    state
        .db
        .upsert_user_account_settings(&user.0, settings.clone())
        .await?;

    Ok(Json(settings))
}

async fn update_profile(
    State(state): State<AuthSqlState>,
    user: AuthenticatedUser,
    Json(body): Json<UpdateProfileRequest>,
) -> Result<Json<ProfileResponse>, AppError> {
    if let Some(ref email) = body.email {
        if !email.is_empty() && !email.contains('@') {
            return Err(AppError::BadRequest("invalid email address".to_string()));
        }
    }

    let db_user = state
        .db
        .load_user_by_id(&user.0)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    let effective_tier = db_user.effective_subscription_tier(Utc::now());
    let access_grants = db_user.effective_access_grants(Utc::now());
    let first_name = body.first_name.map(|value| value.trim().to_string()).or(db_user.first_name);
    let last_name = body.last_name.map(|value| value.trim().to_string()).or(db_user.last_name);
    let email = body.email.map(|value| value.trim().to_string()).or(db_user.email);
    let username = db_user.username;
    let subscription_current_period_end = db_user.subscription_current_period_end;

    state
        .db
        .update_user_profile(
            &user.0,
            UserProfileUpdate {
                first_name: first_name.clone(),
                last_name: last_name.clone(),
                email: email.clone(),
            },
        )
        .await?;

    Ok(Json(ProfileResponse {
        username,
        first_name,
        last_name,
        email,
        subscription_tier: effective_tier.as_str().to_string(),
        storage_limit_bytes: effective_tier.storage_limit_bytes(),
        subscription_current_period_end,
        access_grants,
    }))
}

async fn delete_profile(
    State(state): State<AuthSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_user = state
        .db
        .load_user_by_id(&user.0)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    let maps = state.db.list_mind_maps(&user.0).await?;
    delete_owned_blobs(&state.db, &state.minio, &maps).await?;
    state.db.delete_user(&user.0).await?;

    tracing::info!(
        "Deleted SQL-backed account '{}' with {} vault(s)",
        db_user.username,
        maps.len()
    );

    Ok(Json(serde_json::json!({
        "message": "account deleted",
        "deleted_vaults": maps.len(),
    })))
}

async fn delete_owned_blobs(
    store: &DynSqlStore,
    minio: &MinioClient,
    maps: &[StoredMindMap],
) -> Result<(), AppError> {
    for map in maps {
        let attachments = store.list_mind_map_attachments(&map.id).await?;
        for attachment in attachments {
            match minio.delete_object(&attachment.s3_key).await {
                Ok(()) | Err(AppError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        let shares = store.list_mind_map_shares(&map.id).await?;
        for share in shares {
            let share_attachments = store.list_mind_map_share_attachments(&share.id).await?;
            for attachment in share_attachments {
                match minio.delete_object(&attachment.s3_key).await {
                    Ok(()) | Err(AppError::NotFound(_)) => {}
                    Err(error) => return Err(error),
                }
            }

            match minio.delete_object(&share.s3_key).await {
                Ok(()) | Err(AppError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        match minio.delete_object(&map.minio_object_key).await {
            Ok(()) | Err(AppError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }

    Ok(())
}

fn known_version_ids(
    version_history: &[VersionSnapshot],
    current_version_id: Option<&str>,
) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut version_ids = Vec::new();

    for snapshot in version_history.iter().rev() {
        if seen.insert(snapshot.version_id.clone()) {
            version_ids.push(snapshot.version_id.clone());
        }
    }

    if let Some(version_id) = current_version_id {
        if seen.insert(version_id.to_string()) {
            version_ids.insert(0, version_id.to_string());
        }
    }

    version_ids
}

async fn load_map_share_storage_bytes(
    db: &DynSqlStore,
    map_id: &str,
) -> Result<i64, AppError> {
    let shares = db.list_mind_map_shares(map_id).await?;
    let mut share_bytes = 0_i64;

    for share in shares {
        if share.status == crate::models::share::ShareStatus::Available {
            share_bytes += share.size_bytes;
        }

        let attachments = db.list_mind_map_share_attachments(&share.id).await?;
        share_bytes += attachments
            .into_iter()
            .filter(|attachment| attachment.status == AttachmentStatus::Available)
            .map(|attachment| attachment.size_bytes)
            .sum::<i64>();
    }

    Ok(share_bytes)
}

/// POST /api/auth/rotate-credentials
///
/// Atomically updates the user's auth hash, argon2 parameters, and both wrapped
/// private keys, then re-encrypts every vault title/note in the same DB
/// transaction.  Returns new JWT tokens so the session stays valid.
///
/// Safety properties:
/// - `current_auth_token` re-verifies the current password even over an active
///   JWT session (prevents a stolen token from rotating credentials).
/// - The server enforces that the bundle covers EVERY vault owned by the user.
///   If any vault is absent the request is rejected before the DB is touched.
/// - The DB transaction is all-or-nothing: either all credential + title updates
///   commit together or the database is left completely unchanged.
/// - Vault blobs in object storage are never touched here because they are
///   KEM-encrypted to the user's key-pair, which is unchanged during rotation.
///   All historical blob versions remain decryptable after a successful rotation.
async fn rotate_credentials(
    State(state): State<AuthSqlState>,
    auth: AuthenticatedUser,
    Json(body): Json<RotateCredentialsRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // ── Input validation ──────────────────────────────────────────────────────
    if body.new_auth_token.len() != 64
        || !body.new_auth_token.chars().all(|c: char| c.is_ascii_hexdigit())
    {
        return Err(AppError::BadRequest(
            "new_auth_token must be a 64-character hex string".to_string(),
        ));
    }
    if body.new_argon2_salt.is_empty() {
        return Err(AppError::BadRequest("new_argon2_salt is required".to_string()));
    }

    // ── Load user + re-verify current password ────────────────────────────────
    let db_user = state
        .db
        .load_user_by_id(&auth.0)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    verify_auth_token(&body.current_auth_token, &db_user.auth_hash)?;

    // Prevent version skew: the client must agree on the current key_version.
    if body.new_key_version != db_user.key_version + 1 {
        return Err(AppError::BadRequest(format!(
            "new_key_version must be {} (current {} + 1)",
            db_user.key_version + 1,
            db_user.key_version,
        )));
    }

    // ── Complete-coverage check ───────────────────────────────────────────────
    // Every vault owned by the user must appear in updated_vaults.  A missing
    // vault would retain its old title ciphertext (wrong key) after rotation.
    let all_vaults = state.db.list_mind_maps(&auth.0).await?;
    let submitted_ids: std::collections::HashSet<&str> = body
        .updated_vaults
        .iter()
        .map(|v| v.id.as_str())
        .collect();
    let missing: Vec<&str> = all_vaults
        .iter()
        .filter(|v| !submitted_ids.contains(v.id.as_str()))
        .map(|v| v.id.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(AppError::BadRequest(format!(
            "rotation bundle is incomplete — {} vault(s) missing",
            missing.len(),
        )));
    }

    // ── Execute atomic rotation ───────────────────────────────────────────────
    let updated_vaults = body
        .updated_vaults
        .into_iter()
        .map(|v| RotateVaultEntry {
            id: v.id,
            title_encrypted: v.title_encrypted,
            vault_note_encrypted: v.vault_note_encrypted,
        })
        .collect();

    state
        .db
        .rotate_user_credentials(
            &auth.0,
            RotateCredentialsUpdate {
                new_auth_token: body.new_auth_token,
                new_argon2_salt: body.new_argon2_salt,
                new_argon2_params: body.new_argon2_params,
                new_classical_priv_encrypted: body.new_classical_priv_encrypted,
                new_pq_priv_encrypted: body.new_pq_priv_encrypted,
                new_key_version: body.new_key_version,
                updated_vaults,
            },
        )
        .await?;

    // ── Re-issue tokens ───────────────────────────────────────────────────────
    // The existing JWT remains valid until expiry (stateless), but we return
    // fresh tokens so the client session stays seamless.
    let access_token = state.jwt.issue_access_token(&auth.0)?;
    let refresh_token = state.jwt.issue_refresh_token(&auth.0)?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "access_token": access_token,
        "refresh_token": refresh_token,
    })))
}

fn hash_auth_token(auth_token: &str) -> Result<String, AppError> {
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
        Argon2,
    };

    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(auth_token.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(format!("argon2 hash error: {e}")))?;

    Ok(hash.to_string())
}

fn verify_auth_token(auth_token: &str, stored_hash: &str) -> Result<(), AppError> {
    use argon2::{
        password_hash::{PasswordHash, PasswordVerifier},
        Argon2,
    };

    let parsed_hash = PasswordHash::new(stored_hash)
        .map_err(|e| AppError::Internal(format!("argon2 parse error: {e}")))?;

    Argon2::default()
        .verify_password(auth_token.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::Unauthorized("invalid credentials".to_string()))
}

fn normalize_bounded_string(value: String, field: &str, max_len: usize) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(format!("{field} cannot be empty")));
    }
    if trimmed.len() > max_len {
        return Err(AppError::BadRequest(format!("{field} is too long")));
    }
    Ok(trimmed.to_string())
}

fn normalize_choice(value: String, field: &str, allowed: &[&str]) -> Result<String, AppError> {
    let normalized = normalize_bounded_string(value, field, 64)?;
    if !allowed.iter().any(|candidate| *candidate == normalized) {
        return Err(AppError::BadRequest(format!(
            "{field} must be one of: {}",
            allowed.join(", ")
        )));
    }
    Ok(normalized)
}