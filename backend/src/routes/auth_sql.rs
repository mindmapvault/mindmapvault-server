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
    db::{s3::S3Store, sql_store::{
        DynSqlStore, NewUser, RotateAttachmentEntry, RotateCredentialsUpdate, RotateVaultEntry,
        StoredMindMap, UserProfileUpdate,
    }},
    error::AppError,
    middleware::{
        auth::{AuthenticatedUser, JwtService, KeyVersionCache},
        client_ip::ClientIp,
        throttle::AuthThrottle,
    },
    models::{
        access::{AccessSource, SubscriptionMode, UiSurface, UserAccessGrant},
        attachment::AttachmentStatus,
        instance_settings::InstanceSettingsHandle,
        invite::normalize_invite_code,
        mindmap::stored_version_bytes,
        settings::{UpdateUserAccountSettingsRequest, UserAccountSettings},
        user::{
            AccountCapabilitiesResponse, AccountStorageResponse, KeyBundleResponse,
            LoginRequest, LoginResponse, ProfileResponse, RegisterRequest,
            RotateCredentialsRequest, SaltResponse, SubscriptionSummaryResponse,
            SubscriptionTier, UpdateProfileRequest, MAX_UPLOAD_BODY_BYTES,
        },
    },
};

#[derive(Clone)]
pub struct AuthSqlState {
    pub db: DynSqlStore,
    pub storage: S3Store,
    pub jwt: Arc<JwtService>,
    pub settings: InstanceSettingsHandle,
    pub throttle: Arc<AuthThrottle>,
    pub key_versions: KeyVersionCache,
}

impl FromRef<AuthSqlState> for Arc<JwtService> {
    fn from_ref(state: &AuthSqlState) -> Self {
        state.jwt.clone()
    }
}

impl FromRef<AuthSqlState> for DynSqlStore {
    fn from_ref(state: &AuthSqlState) -> Self {
        state.db.clone()
    }
}

impl FromRef<AuthSqlState> for KeyVersionCache {
    fn from_ref(state: &AuthSqlState) -> Self {
        state.key_versions.clone()
    }
}

impl FromRef<AuthSqlState> for InstanceSettingsHandle {
    fn from_ref(state: &AuthSqlState) -> Self {
        state.settings.clone()
    }
}

pub fn router(state: AuthSqlState) -> Router {
    Router::new()
        .route("/salt", get(get_salt))
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/keys", get(get_keys))
        .route("/rotation-manifest", get(rotation_manifest))
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

/// Counts one request against the caller's per-minute auth allowance.
///
/// Applied to the three unauthenticated routes — salt lookup, register and
/// login — because those are the ones a stranger can call in a loop. The rest
/// of this router needs a valid token first.
fn count_auth_request(state: &AuthSqlState, client_ip: ClientIp) -> Result<(), AppError> {
    let limit = state.settings.get().auth_rate_limit_per_minute;
    state
        .throttle
        .check_address(client_ip.0, limit)
        .map_err(|retry_after| {
            AppError::TooManyRequests(
                "too many attempts; please wait and try again".to_string(),
                retry_after.as_secs().max(1),
            )
        })
}

async fn get_salt(
    State(state): State<AuthSqlState>,
    client_ip: ClientIp,
    Query(q): Query<SaltQuery>,
) -> Result<Json<SaltResponse>, AppError> {
    count_auth_request(&state, client_ip)?;

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
    client_ip: ClientIp,
    Json(body): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    count_auth_request(&state, client_ip)?;

    if body.username.trim().is_empty() {
        return Err(AppError::BadRequest("username cannot be empty".to_string()));
    }
    if body.username.len() > 64 {
        return Err(AppError::BadRequest("username too long".to_string()));
    }
    if body.auth_token.is_empty() || body.argon2_salt.is_empty() {
        return Err(AppError::BadRequest("missing required crypto fields".to_string()));
    }

    let username = body.username.trim().to_string();
    let now = Utc::now();

    // On a closed instance an invite is the only way through. It is claimed
    // here — before the account is written — so two people racing one code
    // cannot both use it; if the sign-up then fails, it is handed back below.
    let claimed_invite = if state.settings.get().registration_enabled {
        None
    } else {
        let code = body
            .invite_code
            .as_deref()
            .map(normalize_invite_code)
            .filter(|code| !code.is_empty())
            .ok_or_else(|| {
                AppError::Forbidden("registration is closed on this server".to_string())
            })?;

        let invite = state
            .db
            .claim_registration_invite(&code, &username, now)
            .await?
            .ok_or_else(|| {
                // One message for wrong, spent and expired alike: telling them
                // apart would let someone probe which codes exist.
                AppError::Forbidden("this invite code is not valid".to_string())
            })?;

        Some(invite)
    };

    // From here on any failure has to give the invite back.
    let release_invite = |error: AppError| async {
        if let Some(invite) = claimed_invite.as_ref() {
            if let Err(release_error) = state.db.release_registration_invite(&invite.id).await {
                tracing::warn!(?release_error, "could not release invite after a failed sign-up");
            }
        }
        error
    };

    if state.db.load_user_by_username(&username).await?.is_some() {
        return Err(release_invite(AppError::Conflict("username already taken".to_string())).await);
    }

    let user_id = Uuid::new_v4().to_string();
    let auth_hash = match hash_auth_token(&body.auth_token) {
        Ok(hash) => hash,
        Err(error) => return Err(release_invite(error).await),
    };

    let created = state
        .db
        .create_user(NewUser {
            id: user_id,
            username: username.clone(),
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
                source: AccessSource::LegacyBase,
                granted_at: now,
                expires_at: None,
                note: Some("Default encrypted app access".to_string()),
            }],
        })
        .await;

    if let Err(error) = created {
        return Err(release_invite(error).await);
    }

    if claimed_invite.is_some() {
        tracing::info!("new user registered with an invite");
    } else {
        tracing::info!("new user registered");
    }
    Ok(Json(serde_json::json!({ "message": "registered successfully" })))
}

async fn login(
    State(state): State<AuthSqlState>,
    client_ip: ClientIp,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    count_auth_request(&state, client_ip)?;

    if body.username.is_empty() || body.auth_token.is_empty() {
        return Err(AppError::BadRequest("username and auth_token are required".to_string()));
    }

    let settings = state.settings.get();

    // Checked before the account is even looked up, so a locked-out attacker
    // learns nothing further and costs the database nothing.
    if let Some(remaining) = state.throttle.lockout_remaining(&body.username) {
        return Err(AppError::TooManyRequests(
            "too many failed sign-in attempts; please wait and try again".to_string(),
            remaining.as_secs().max(1),
        ));
    }

    let user = match state.db.load_user_by_username(&body.username).await? {
        Some(user) => user,
        None => {
            // An unknown username counts as a failure too. Skipping it here
            // would make a wrong username measurably cheaper than a wrong
            // password, which is a username oracle.
            state.throttle.record_failure(
                &body.username,
                settings.failed_login_threshold,
                settings.failed_login_lockout_minutes,
            );
            return Err(AppError::Unauthorized("invalid credentials".to_string()));
        }
    };

    if user.is_locked {
        return Err(AppError::Unauthorized("account is locked".to_string()));
    }

    if verify_auth_token(&body.auth_token, &user.auth_hash).is_err() {
        state.throttle.record_failure(
            &body.username,
            settings.failed_login_threshold,
            settings.failed_login_lockout_minutes,
        );
        return Err(AppError::Unauthorized("invalid credentials".to_string()));
    }

    state.throttle.record_success(&body.username);

    let access_token = state.jwt.issue_access_token(&user.id, user.key_version)?;
    let refresh_token = state.jwt.issue_refresh_token(&user.id, user.key_version)?;
    state.key_versions.set(&user.id, user.key_version);

    tracing::info!(user_id = %user.id, "user logged in");

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
    // A refresh token minted before a password rotation belongs to a device
    // that still derives keys from the old password; renewing it would keep
    // that device writing undecryptable ciphertext for up to the refresh
    // lifetime. Tokens without the claim predate it — refuse those too, which
    // costs each device one re-login after the upgrade.
    if claims.kv != Some(user.key_version) {
        return Err(AppError::Unauthorized(
            "this session was signed in before the password changed — sign in again".to_string(),
        ));
    }
    state.key_versions.set(&user.id, user.key_version);
    let access_token = state.jwt.issue_access_token(&claims.sub, user.key_version)?;
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
        total_bytes += stored_version_bytes(&map.version_history);
        let attachments = state.db.list_mind_map_attachments(&map.id).await?;
        let available: Vec<_> = attachments
            .iter()
            .filter(|a| a.status == AttachmentStatus::Available)
            .collect();
        let all_map_attachment_bytes: i64 = available.iter().map(|a| a.size_bytes).sum();
        let (primary_count, primary_bytes) = available.iter().fold((0usize, 0i64), |acc, a| {
            let is_preview = a
                .encryption_meta
                .as_ref()
                .and_then(|m| m.get("cryptmind_role"))
                .and_then(|r| r.as_str())
                == Some("preview");
            if is_preview { acc } else { (acc.0 + 1, acc.1 + a.size_bytes) }
        });
        attachment_count += primary_count;
        attachment_bytes += primary_bytes;
        total_bytes += all_map_attachment_bytes;
    }

    // The instance setting is the authority here, not the subscription tier.
    // Reporting the tier's cap would tell a self-hosted user they are over a
    // 25 MB "free plan" limit that this server does not have and never
    // enforced.
    let plan_limit_bytes = state
        .settings
        .get()
        .storage_limit()
        .unwrap_or(i64::MAX);
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
    let settings = state.settings.get();

    Ok(Json(AccountCapabilitiesResponse {
        plan_tier: subscription_tier.as_str().to_string(),
        // Both caps come from the instance settings; a self-hosted install has
        // no plan to be limited by. The attachment cap still cannot exceed what
        // the transport will carry, so it is clamped rather than advertised as
        // something an upload would then fail on.
        storage_limit_bytes: settings.storage_limit().unwrap_or(i64::MAX),
        max_attachment_size_bytes: settings
            .attachment_limit()
            .unwrap_or(MAX_UPLOAD_BODY_BYTES as i64)
            .min(MAX_UPLOAD_BODY_BYTES as i64),
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
    // Existence check — returns 404 if the authenticated user is no longer in the DB.
    state
        .db
        .load_user_by_id(&user.0)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    let maps = state.db.list_mind_maps(&user.0).await?;
    delete_owned_blobs(&state.db, &state.storage, &maps).await?;
    state.db.delete_user(&user.0).await?;

    tracing::info!(user_id = %user.0, vault_count = maps.len(), "account deleted");

    Ok(Json(serde_json::json!({
        "message": "account deleted",
        "deleted_vaults": maps.len(),
    })))
}

async fn delete_owned_blobs(
    store: &DynSqlStore,
    storage: &S3Store,
    maps: &[StoredMindMap],
) -> Result<(), AppError> {
    for map in maps {
        let attachments = store.list_mind_map_attachments(&map.id).await?;
        for attachment in attachments {
            match storage.delete_object(&attachment.s3_key).await {
                Ok(()) | Err(AppError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        match storage.delete_object(&map.object_key).await {
            Ok(()) | Err(AppError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }

    Ok(())
}


/// GET /api/auth/rotation-manifest
///
/// One snapshot of everything a password rotation must rewrite: the key
/// bundle, every vault's title and note, and the `encryption_meta` of every
/// attachment whose file key is wrapped with a password-derived key. The
/// attachment list uses the same predicate as the rotation transaction's
/// coverage check, so a client that rewraps exactly this list submits a
/// complete bundle — unless something changed in between, which the
/// in-transaction check then catches.
async fn rotation_manifest(
    State(state): State<AuthSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let db_user = state
        .db
        .load_user_by_id(&user.0)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    let vaults = state.db.list_mind_maps(&user.0).await?;
    let attachments = state.db.list_rotation_attachments(&user.0).await?;

    Ok(Json(serde_json::json!({
        "key_version": db_user.key_version,
        "argon2_salt": db_user.argon2_salt,
        "argon2_params": db_user.argon2_params,
        "classical_priv_encrypted": db_user.classical_priv_encrypted,
        "pq_priv_encrypted": db_user.pq_priv_encrypted,
        "vaults": vaults.iter().map(|v| serde_json::json!({
            "id": v.id,
            "title_encrypted": v.title_encrypted,
            "vault_note_encrypted": v.vault_note_encrypted,
        })).collect::<Vec<_>>(),
        "attachments": attachments.iter().map(|a| serde_json::json!({
            "id": a.id,
            "map_id": a.map_id,
            "encryption_meta": a.encryption_meta,
        })).collect::<Vec<_>>(),
    })))
}

/// POST /api/auth/rotate-credentials
///
/// Atomically updates the user's auth hash, argon2 parameters and both
/// wrapped private keys, re-encrypts every vault title/note, and re-wraps
/// every attachment file key — one DB transaction on a dedicated connection.
/// Returns new JWT tokens (carrying the new key version) so the rotating
/// session stays valid; every other session fails the key-version check and
/// must sign in again with the new password.
///
/// Safety properties:
/// - `current_auth_token` re-verifies the current password even over an active
///   JWT session (prevents a stolen token from rotating credentials).
/// - Version agreement and complete coverage — every vault AND every
///   attachment with a wrapped file key — are enforced INSIDE the transaction,
///   under a `FOR UPDATE` lock on the user row, in `rotate_user_credentials`.
///   A miss on either set aborts with the database unchanged.
/// - Vault blobs in object storage are never touched here because they are
///   KEM-encrypted to the user's key-pair, which is unchanged during rotation.
///   All historical blob versions remain decryptable after a successful
///   rotation. Attachment blobs are untouched too — only their ~60-byte
///   wrapped file key in `encryption_meta` changes.
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

    // Shape checks only — version agreement and complete coverage of vaults
    // and attachments are enforced inside the rotation transaction, where
    // they hold against the database as it is at commit time rather than as
    // it was when this request was built.
    for attachment in &body.updated_attachments {
        if attachment.id.is_empty() {
            return Err(AppError::BadRequest("attachment id must not be empty".to_string()));
        }
        if attachment.wrapped_key_b64.is_empty() || attachment.wrapped_key_b64.len() > 1024 {
            return Err(AppError::BadRequest(format!(
                "wrapped_key_b64 for attachment {} is empty or implausibly large",
                attachment.id,
            )));
        }
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
    let updated_attachments = body
        .updated_attachments
        .into_iter()
        .map(|a| RotateAttachmentEntry {
            id: a.id,
            wrapped_key_b64: a.wrapped_key_b64,
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
                updated_attachments,
            },
        )
        .await?;

    // Publish the new key version so every other session's next WRITE is
    // refused (VerifiedWriter) instead of storing old-key ciphertext.
    state.key_versions.set(&auth.0, body.new_key_version);

    // ── Re-issue tokens ───────────────────────────────────────────────────────
    // The rotating session gets tokens carrying the new key version, so it
    // alone continues seamlessly.
    let access_token = state.jwt.issue_access_token(&auth.0, body.new_key_version)?;
    let refresh_token = state.jwt.issue_refresh_token(&auth.0, body.new_key_version)?;

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