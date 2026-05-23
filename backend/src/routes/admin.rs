use axum::{
    extract::{Path, State},
    http::{header, HeaderMap},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::{HashMap, HashSet}, time::Duration};
use uuid::Uuid;

use crate::{
    db::{
        minio::MinioClient,
        sql_store::{
            AdminUserAdminUpdate, AdminUserRecord, DynSqlStore,
            ManualSubscriptionUpdate, StoredMindMap,
        },
    },
    error::AppError,
    models::{
        access::UserAccessGrant,
        admin_audit::AdminAuditEvent,
        attachment::AttachmentStatus,
        user::SubscriptionTier,
    },
};

const DEFAULT_AUDIT_LIMIT: usize = 50;

#[derive(Clone)]
pub struct AdminState {
    pub db: DynSqlStore,
    pub minio: MinioClient,
    pub admin_api_token: String,
}

pub fn router(state: AdminState) -> Router {
    Router::new()
        .route("/overview", get(get_overview))
    .route("/users/{id}/account-lock", post(set_user_lock))
    .route("/users/{id}/admin-details", post(update_user_admin_details))
    .route("/users/{id}/access-grants", post(update_user_access_grants))
    .route("/users/{id}/plan-override", post(update_user_plan_override))
    .route("/users/{id}/delete-account", post(delete_user_account))
        .with_state(state)
}

#[derive(Serialize)]
struct AdminOverviewResponse {
    generated_at: DateTime<Utc>,
    metrics: AdminMetrics,
    users: Vec<AdminUserSummary>,
    audit_events: Vec<AdminAuditSummary>,
}

#[derive(Serialize)]
struct AdminMetrics {
    total_users: usize,
    free_users: usize,
    paid_users: usize,
    locked_users: usize,
    active_subscriptions: usize,
    total_vaults: usize,
    total_used_bytes: i64,
}

#[derive(Serialize)]
struct AdminUserSummary {
    id: String,
    username: String,
    created_at: DateTime<Utc>,
    subscription_tier: &'static str,
    effective_subscription_tier: &'static str,
    plan_source: &'static str,
    stripe_customer_id: Option<String>,
    stripe_subscription_id: Option<String>,
    stripe_subscription_status: Option<String>,
    subscription_current_period_end: Option<DateTime<Utc>>,
    first_name: Option<String>,
    last_name: Option<String>,
    email: Option<String>,
    is_locked: bool,
    locked_reason: Option<String>,
    admin_note: Option<String>,
    manual_subscription_tier: Option<&'static str>,
    manual_subscription_expires_at: Option<DateTime<Utc>>,
    manual_subscription_reason: Option<String>,
    manual_subscription_granted_by: Option<String>,
    access_grants: Vec<UserAccessGrant>,
    vault_count: usize,
    used_bytes: i64,
    storage_limit_bytes: i64,
}

#[derive(Serialize)]
struct AdminAuditSummary {
    public_id: String,
    entity_type: String,
    entity_id: String,
    action_type: String,
    summary: String,
    detail: Option<String>,
    actor: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Default)]
struct UserStorageSummary {
    vault_count: usize,
    used_bytes: i64,
}

#[derive(Deserialize)]
struct AdminLockUserRequest {
    locked: bool,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
struct AdminDeleteUserRequest {
    delete_all_data: bool,
}

#[derive(Deserialize)]
struct AdminUserDetailsRequest {
    #[serde(default)]
    admin_note: Option<String>,
    #[serde(default)]
    locked_reason: Option<String>,
}

#[derive(Deserialize)]
struct AdminPlanOverrideRequest {
    #[serde(default)]
    manual_subscription_tier: Option<String>,
    #[serde(default)]
    manual_subscription_expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
struct AdminAccessGrantsRequest {
    #[serde(default)]
    access_grants: Vec<UserAccessGrant>,
}

async fn get_overview(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Result<Json<AdminOverviewResponse>, AppError> {
    authorize_admin(&state, &headers).await?;
    Ok(Json(build_overview(&state).await?))
}

async fn set_user_lock(
    State(state): State<AdminState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AdminLockUserRequest>,
) -> Result<Json<AdminOverviewResponse>, AppError> {
    authorize_admin(&state, &headers).await?;
    let reason = normalize_optional(body.reason);

    {
        let store = &state.db;
        let user = store
            .load_user_by_id(&user_id)
            .await?
            .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;
        store.set_user_locked(&user_id, body.locked).await?;
        store
            .update_user_admin_fields(
                &user_id,
                AdminUserAdminUpdate {
                    admin_note: user.admin_note,
                    locked_reason: reason.clone(),
                },
            )
            .await?;
    }

    write_audit_event(
        &state,
        make_audit_event(
            "user",
            &user_id,
            if body.locked { "user_locked" } else { "user_unlocked" },
            if body.locked {
                format!("Locked account {user_id}")
            } else {
                format!("Unlocked account {user_id}")
            },
            reason,
        ),
    )
    .await?;

    Ok(Json(build_overview(&state).await?))
}

async fn update_user_admin_details(
    State(state): State<AdminState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AdminUserDetailsRequest>,
) -> Result<Json<AdminOverviewResponse>, AppError> {
    authorize_admin(&state, &headers).await?;

    let admin_note = normalize_optional(body.admin_note);
    let locked_reason = normalize_optional(body.locked_reason);

    {
        let store = &state.db;
        if store.load_user_by_id(&user_id).await?.is_none() {
            return Err(AppError::NotFound("user not found".to_string()));
        }
        store
            .update_user_admin_fields(
                &user_id,
                AdminUserAdminUpdate {
                    admin_note: admin_note.clone(),
                    locked_reason: locked_reason.clone(),
                },
            )
            .await?;
    }

    write_audit_event(
        &state,
        make_audit_event(
            "user",
            &user_id,
            "user_details_updated",
            format!("Updated admin details for {user_id}"),
            admin_note.clone().or(locked_reason.clone()),
        ),
    )
    .await?;

    Ok(Json(build_overview(&state).await?))
}

async fn update_user_plan_override(
    State(state): State<AdminState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AdminPlanOverrideRequest>,
) -> Result<Json<AdminOverviewResponse>, AppError> {
    authorize_admin(&state, &headers).await?;

    let manual_tier = body
        .manual_subscription_tier
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(SubscriptionTier::from_str);
    let reason = normalize_optional(body.reason);

    {
        let store = &state.db;
        if store.load_user_by_id(&user_id).await?.is_none() {
            return Err(AppError::NotFound("user not found".to_string()));
        }
        store
            .update_user_manual_subscription(
                &user_id,
                ManualSubscriptionUpdate {
                    manual_subscription_tier: manual_tier.clone(),
                    manual_subscription_expires_at: body.manual_subscription_expires_at,
                    manual_subscription_reason: reason.clone(),
                    manual_subscription_granted_by: Some("admin".to_string()),
                },
            )
            .await?;
    }

    write_audit_event(
        &state,
        make_audit_event(
            "user",
            &user_id,
            if manual_tier.is_some() { "manual_plan_granted" } else { "manual_plan_cleared" },
            if let Some(tier) = manual_tier.as_ref() {
                format!("Set manual plan override for {user_id} to {}", tier.as_str())
            } else {
                format!("Cleared manual plan override for {user_id}")
            },
            reason,
        ),
    )
    .await?;

    Ok(Json(build_overview(&state).await?))
}

async fn update_user_access_grants(
    State(state): State<AdminState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AdminAccessGrantsRequest>,
) -> Result<Json<AdminOverviewResponse>, AppError> {
    authorize_admin(&state, &headers).await?;

    let access_grants = normalize_access_grants(body.access_grants);

    {
        let store = &state.db;
        if store.load_user_by_id(&user_id).await?.is_none() {
            return Err(AppError::NotFound("user not found".to_string()));
        }
        store
            .update_user_access_grants(&user_id, access_grants.clone())
            .await?;
    }

    write_audit_event(
        &state,
        make_audit_event(
            "user",
            &user_id,
            "user_access_grants_updated",
            format!("Updated access grants for {user_id}"),
            Some(format!("{} active grant(s) saved", access_grants.len())),
        ),
    )
    .await?;

    Ok(Json(build_overview(&state).await?))
}

async fn delete_user_account(
    State(state): State<AdminState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AdminDeleteUserRequest>,
) -> Result<Json<AdminOverviewResponse>, AppError> {
    authorize_admin(&state, &headers).await?;

    if !body.delete_all_data {
        return Err(AppError::BadRequest("delete_all_data must be true".to_string()));
    }

    {
        delete_sql_user_account(&state.db, &state.minio, &user_id).await?;
    }

    write_audit_event(
        &state,
        make_audit_event(
            "user",
            &user_id,
            "user_deleted",
            format!("Deleted account {user_id} and all vault data"),
            None,
        ),
    )
    .await?;

    Ok(Json(build_overview(&state).await?))
}

async fn build_overview(state: &AdminState) -> Result<AdminOverviewResponse, AppError> {
    let store = &state.db;
    let users = store.list_admin_users().await?;
    let audit_events = store.list_admin_audit_events(DEFAULT_AUDIT_LIMIT).await?;

    let storage = load_sql_user_storage(store, &state.minio, &users).await?;

    let total_vaults: usize = storage.iter().map(|summary| summary.vault_count).sum();
    let total_used_bytes: i64 = storage.iter().map(|summary| summary.used_bytes).sum();
    let now = Utc::now();

    let metrics = AdminMetrics {
        total_users: users.len(),
        free_users: users
            .iter()
            .filter(|user| matches!(user.effective_subscription_tier(now), SubscriptionTier::Free))
            .count(),
        paid_users: users
            .iter()
            .filter(|user| matches!(user.effective_subscription_tier(now), SubscriptionTier::Paid))
            .count(),
        locked_users: users.iter().filter(|user| user.is_locked).count(),
        active_subscriptions: users
            .iter()
            .filter(|user| matches!(user.stripe_subscription_status.as_deref(), Some("active") | Some("trialing")))
            .count(),
        total_vaults,
        total_used_bytes,
    };

    Ok(AdminOverviewResponse {
        generated_at: now,
        metrics,
        users: users
            .into_iter()
            .zip(storage.into_iter())
            .map(|(user, storage)| map_admin_user(user, storage, now))
            .collect(),
        audit_events: audit_events.into_iter().map(map_admin_audit).collect(),
    })
}

async fn authorize_admin(state: &AdminState, headers: &HeaderMap) -> Result<(), AppError> {
    let expected = state.admin_api_token.trim();
    if expected.is_empty() {
        return Err(AppError::Unauthorized("admin api is not configured".to_string()));
    }

    let provided = bearer_token(headers)
        .ok_or_else(|| AppError::Unauthorized("missing admin bearer token".to_string()))?;

    if provided != expected {
        return Err(AppError::Unauthorized("invalid admin bearer token".to_string()));
    }

    Ok(())
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let header = headers.get(header::AUTHORIZATION)?.to_str().ok()?.trim();
    header.strip_prefix("Bearer ").map(str::trim).filter(|value| !value.is_empty())
}



async fn load_sql_user_storage(
    store: &DynSqlStore,
    minio: &MinioClient,
    users: &[AdminUserRecord],
) -> Result<Vec<UserStorageSummary>, AppError> {
    let mut user_maps = Vec::with_capacity(users.len());
    let mut object_keys = HashSet::new();

    for user in users {
        let maps = store.list_mind_maps(&user.id).await?;
        object_keys.extend(maps.iter().map(|map| map.minio_object_key.clone()));
        user_maps.push(maps);
    }

    let size_totals = load_bucket_size_totals(minio, &object_keys).await;
    let mut storage = Vec::with_capacity(user_maps.len());
    for maps in user_maps {
        let mut used_bytes = 0_i64;
        for map in &maps {
            used_bytes += size_totals.get(&map.minio_object_key).copied().unwrap_or(0);
            let attachments = store.list_mind_map_attachments(&map.id).await?;
            used_bytes += attachments
                .iter()
                .filter(|attachment| attachment.status == AttachmentStatus::Available)
                .map(|attachment| attachment.size_bytes)
                .sum::<i64>();
        }

        storage.push(UserStorageSummary {
            vault_count: maps.len(),
            used_bytes,
        });
    }

    Ok(storage)
}

async fn load_bucket_size_totals(
    minio: &MinioClient,
    object_keys: &HashSet<String>,
) -> HashMap<String, i64> {
    if object_keys.is_empty() {
        return HashMap::new();
    }

    match tokio::time::timeout(
        Duration::from_secs(10),
        minio.list_version_size_totals_for_keys(object_keys),
    )
    .await
    {
        Ok(Ok(totals)) => totals,
        Ok(Err(error)) => {
            tracing::warn!(
                "Admin overview storage totals fallback triggered after MinIO error: {}",
                error
            );
            HashMap::new()
        }
        Err(_) => {
            tracing::warn!(
                "Admin overview storage totals fallback triggered after MinIO timeout"
            );
            HashMap::new()
        }
    }
}



async fn delete_sql_user_account(
    store: &DynSqlStore,
    minio: &MinioClient,
    user_id: &str,
) -> Result<(), AppError> {
    let db_user = store
        .load_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;
    let maps = store.list_mind_maps(user_id).await?;
    delete_owned_blobs(store, minio, &maps).await?;
    store.delete_user(user_id).await?;

    tracing::info!(
        "Admin deleted SQL-backed account '{}' with {} vault(s)",
        db_user.username,
        maps.len()
    );

    Ok(())
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

        match minio.delete_object(&map.minio_object_key).await {
            Ok(()) | Err(AppError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }

    Ok(())
}

async fn write_audit_event(state: &AdminState, event: AdminAuditEvent) -> Result<(), AppError> {
    let store = &state.db;
    store.create_admin_audit_event(event).await?;

    Ok(())
}

fn make_audit_event(
    entity_type: &str,
    entity_id: &str,
    action_type: &str,
    summary: String,
    detail: Option<String>,
) -> AdminAuditEvent {
    AdminAuditEvent {
        id: None,
        public_id: Uuid::new_v4().to_string(),
        entity_type: entity_type.to_string(),
        entity_id: entity_id.to_string(),
        action_type: action_type.to_string(),
        summary,
        detail,
        actor: Some("admin".to_string()),
        created_at: Utc::now(),
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.map(|item| item.trim().to_string()).filter(|item| !item.is_empty())
}

fn normalize_access_grants(access_grants: Vec<UserAccessGrant>) -> Vec<UserAccessGrant> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for mut grant in access_grants {
        grant.note = normalize_optional(grant.note);

        let key = (grant.subscription_mode.clone(), grant.ui_surface.clone());
        if seen.insert(key) {
            normalized.push(grant);
        }
    }

    normalized.sort_by(|left, right| {
        format!("{:?}:{:?}", left.subscription_mode, left.ui_surface)
            .cmp(&format!("{:?}:{:?}", right.subscription_mode, right.ui_surface))
    });

    normalized
}

fn map_admin_user(user: AdminUserRecord, storage: UserStorageSummary, now: DateTime<Utc>) -> AdminUserSummary {
    let effective_tier = user.effective_subscription_tier(now);
    let plan_source = user.effective_plan_source(now);
    let access_grants = user.effective_access_grants(now);
    AdminUserSummary {
        id: user.id,
        username: user.username,
        created_at: user.created_at,
        subscription_tier: user.subscription_tier.as_str(),
        effective_subscription_tier: effective_tier.as_str(),
        plan_source,
        stripe_customer_id: user.stripe_customer_id,
        stripe_subscription_id: user.stripe_subscription_id,
        stripe_subscription_status: user.stripe_subscription_status,
        subscription_current_period_end: user.subscription_current_period_end,
        first_name: user.first_name,
        last_name: user.last_name,
        email: user.email,
        is_locked: user.is_locked,
        locked_reason: user.locked_reason,
        admin_note: user.admin_note,
        manual_subscription_tier: user.manual_subscription_tier.as_ref().map(|value| value.as_str()),
        manual_subscription_expires_at: user.manual_subscription_expires_at,
        manual_subscription_reason: user.manual_subscription_reason,
        manual_subscription_granted_by: user.manual_subscription_granted_by,
        access_grants,
        vault_count: storage.vault_count,
        used_bytes: storage.used_bytes,
        storage_limit_bytes: effective_tier.storage_limit_bytes(),
    }
}

fn map_admin_audit(event: AdminAuditEvent) -> AdminAuditSummary {
    AdminAuditSummary {
        public_id: event.public_id,
        entity_type: event.entity_type,
        entity_id: event.entity_id,
        action_type: event.action_type,
        summary: event.summary,
        detail: event.detail,
        actor: event.actor,
        created_at: event.created_at,
    }
}