use axum::{
    extract::{FromRef, Path, State},
    http::{header, HeaderMap},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::{HashMap, HashSet}, time::Duration};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{
    db::{
        minio::MinioClient,
        sql_store::{AdminUserAdminUpdate, AdminUserRecord, DynSqlStore, StoredMindMap},
    },
    error::AppError,
    middleware::client_ip::ClientIp,
    models::{
        admin_audit::AdminAuditEvent,
        attachment::AttachmentStatus,
        instance_settings::{InstanceSettings, InstanceSettingsHandle, UpdateInstanceSettingsRequest},
        invite::{generate_invite_code, CreateInviteRequest, RegistrationInvite},
        status::{
            disk_usage, process_memory_bytes, BucketStats, DatabaseStats, DependencyHealth,
            DiskUsage, PurgeStatus, PurgeStatusHandle,
        },
        user::MAX_UPLOAD_BODY_BYTES,
    },
};

const DEFAULT_AUDIT_LIMIT: usize = 50;

/// Pages of object versions the status page will walk before giving up and
/// reporting a floor. 1000 versions a page, so 20 pages is 20k — well past a
/// home instance, and bounded enough that one status request cannot stall.
const BUCKET_SCAN_MAX_PAGES: usize = 20;

/// Where the disk starts being worth mentioning, and where it becomes the most
/// important thing on the page.
const DISK_WARN_PERCENT: u8 = 80;
const DISK_CRITICAL_PERCENT: u8 = 92;

/// Bytes in the units a person reads, for warning text. The console formats its
/// own figures; this is only for messages composed server-side.
fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = value as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", value, UNITS[0])
    } else if size < 10.0 {
        format!("{size:.1} {}", UNITS[unit])
    } else {
        format!("{} {}", size.round(), UNITS[unit])
    }
}

#[derive(Clone)]
pub struct AdminState {
    pub db: DynSqlStore,
    pub minio: MinioClient,
    pub admin_api_token: String,
    pub settings: InstanceSettingsHandle,
    pub purge_status: PurgeStatusHandle,
    /// When this process started, for the uptime shown on the status page.
    pub started_at: DateTime<Utc>,
    /// Filesystem whose free space the status page reports.
    pub disk_path: String,
}

impl FromRef<AdminState> for InstanceSettingsHandle {
    fn from_ref(state: &AdminState) -> Self {
        state.settings.clone()
    }
}

pub fn router(state: AdminState) -> Router {
    Router::new()
        .route("/status", get(get_status))
        .route("/overview", get(get_overview))
        .route("/settings", get(get_settings).post(update_settings))
        .route("/invites", get(list_invites).post(create_invite))
        .route("/invites/{id}", delete(revoke_invite))
        .route("/maintenance/purge-shares", post(run_share_purge))
        .route("/users/{id}/account-lock", post(set_user_lock))
        .route("/users/{id}/admin-details", post(update_user_admin_details))
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
    locked_users: usize,
    total_vaults: usize,
    total_used_bytes: i64,
}

#[derive(Serialize)]
struct AdminUserSummary {
    id: String,
    username: String,
    created_at: DateTime<Utc>,
    first_name: Option<String>,
    last_name: Option<String>,
    email: Option<String>,
    is_locked: bool,
    locked_reason: Option<String>,
    admin_note: Option<String>,
    vault_count: usize,
    used_bytes: i64,
    /// The instance-wide cap, repeated per row so the console can draw a usage
    /// bar without knowing about the settings. `None` when nothing is capped.
    storage_limit_bytes: Option<i64>,
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

/// Everything the status page shows: is it healthy, what is it running, how
/// much is in it, and is anything set up in a way that will bite later.
#[derive(Serialize)]
struct AdminStatusResponse {
    generated_at: DateTime<Utc>,
    version: &'static str,
    started_at: DateTime<Utc>,
    uptime_seconds: i64,
    /// The process itself: what it is using on the machine it runs on.
    server: ServerRuntime,
    database: DependencyHealth,
    /// Absent when the database did not answer.
    database_stats: Option<DatabaseStats>,
    object_storage: DependencyHealth,
    storage_bucket: String,
    /// Absent when the object store did not answer, or the scan failed.
    bucket_stats: Option<BucketStats>,
    totals: AdminStatusTotals,
    purge: PurgeStatus,
    /// Things worth telling the operator about their own configuration.
    warnings: Vec<StatusWarning>,
}

#[derive(Serialize)]
struct ServerRuntime {
    /// Resident memory, on platforms that report it.
    memory_bytes: Option<u64>,
    /// Space on the filesystem this process sits on. In the shipped container
    /// that is the Docker host's filesystem, which is normally where the
    /// database and object-storage volumes live too.
    disk: Option<DiskUsage>,
    disk_used_percent: Option<u8>,
}

#[derive(Serialize)]
struct AdminStatusTotals {
    accounts: usize,
    locked_accounts: usize,
    vaults: usize,
    stored_bytes: i64,
    open_invites: usize,
}

#[derive(Serialize)]
struct StatusWarning {
    /// Stable identifier so the console can style or suppress one.
    code: &'static str,
    title: String,
    detail: String,
}

async fn get_status(
    State(state): State<AdminState>,
    headers: HeaderMap,
    client_ip: ClientIp,
) -> Result<Json<AdminStatusResponse>, AppError> {
    authorize_admin(&state, &headers).await?;

    let now = Utc::now();
    let database = measure(|| state.db.health_check()).await;
    let object_storage = measure_storage(&state).await;

    // Only asked for when the dependency answered at all — there is nothing to
    // learn from a size query against something that is already down, and a
    // second timeout would just make the page slower to say so.
    let database_stats = if database.reachable {
        state.db.database_stats().await.ok()
    } else {
        None
    };

    let bucket_stats = if object_storage.reachable {
        match tokio::time::timeout(
            Duration::from_secs(10),
            state.minio.bucket_stats(BUCKET_SCAN_MAX_PAGES),
        )
        .await
        {
            Ok(Ok(stats)) => Some(stats),
            Ok(Err(error)) => {
                tracing::warn!(error, "bucket stats unavailable");
                None
            }
            Err(_) => {
                tracing::warn!("bucket stats timed out");
                None
            }
        }
    } else {
        None
    };

    let disk = disk_usage(&state.disk_path);

    // The counts come from the same overview query the People page uses, so
    // the two pages can never disagree about how much is stored.
    let users = state.db.list_admin_users().await?;
    let storage = load_sql_user_storage(&state.db, &state.minio, &users).await?;
    let invites = state.db.list_registration_invites().await?;
    let settings = state.settings.get();

    let totals = AdminStatusTotals {
        accounts: users.len(),
        locked_accounts: users.iter().filter(|user| user.is_locked).count(),
        vaults: storage.iter().map(|summary| summary.vault_count).sum(),
        stored_bytes: storage.iter().map(|summary| summary.used_bytes).sum(),
        open_invites: invites
            .iter()
            .filter(|invite| invite.status(now) == "open")
            .count(),
    };

    let mut warnings = Vec::new();

    // First, because running out of disk is the failure that loses data rather
    // than merely annoying someone.
    if let Some(usage) = disk.as_ref() {
        let percent = usage.used_percent();
        if percent >= DISK_CRITICAL_PERCENT {
            warnings.push(StatusWarning {
                code: "disk_critical",
                title: format!("The disk is {percent}% full"),
                detail: format!(
                    "Only {} left on {}. Postgres stops accepting writes when it cannot extend a \
                     file, and uploads will start failing. Free space or move the volumes to a \
                     bigger disk now.",
                    format_bytes(usage.available_bytes),
                    usage.path,
                ),
            });
        } else if percent >= DISK_WARN_PERCENT {
            warnings.push(StatusWarning {
                code: "disk_low",
                title: format!("The disk is {percent}% full"),
                detail: format!(
                    "{} left on {}. Worth clearing space or setting a storage limit per account \
                     before it becomes urgent.",
                    format_bytes(usage.available_bytes),
                    usage.path,
                ),
            });
        }
    }

    if settings.registration_enabled && settings.storage_limit().is_none() {
        warnings.push(StatusWarning {
            code: "open_and_unlimited",
            title: "Anyone can sign up, and no account has a storage limit".to_string(),
            detail: "If this server is reachable from the internet, someone who finds it can \
                     create accounts and fill your disk. Turn off sign-ups and hand out invite \
                     codes, or set a storage limit per account."
                .to_string(),
        });
    } else if settings.registration_enabled {
        warnings.push(StatusWarning {
            code: "registration_open",
            title: "Anyone who can reach this server can create an account".to_string(),
            detail: "Fine on a private network. If it is reachable from the internet, turn \
                     sign-ups off and invite people with a code instead."
                .to_string(),
        });
    }

    // Only worth raising when the header is actually arriving: an instance
    // with no proxy in front has nothing to fix here.
    if !settings.trust_proxy_headers && headers.contains_key("x-forwarded-for") {
        warnings.push(StatusWarning {
            code: "proxy_header_ignored",
            title: "There is a proxy in front, but its client addresses are being ignored"
                .to_string(),
            detail: format!(
                "This request arrived with an X-Forwarded-For header but was counted as coming \
                 from {client_ip}. Until you turn on \"Read the client address from \
                 X-Forwarded-For\" in Settings, everyone shares one sign-in allowance."
            ),
        });
    }

    if state.admin_api_token.trim().len() < 24 {
        warnings.push(StatusWarning {
            code: "short_admin_token",
            title: "The admin token is short".to_string(),
            detail: "This one token is all that stands in front of the controls on this page. \
                     Set ADMIN_API_TOKEN to a long random value and restart."
                .to_string(),
        });
    }

    if let Some(error) = state.purge_status.get().last_error {
        warnings.push(StatusWarning {
            code: "purge_failed",
            title: "The last cleanup of expired shares did not finish".to_string(),
            detail: error,
        });
    }

    Ok(Json(AdminStatusResponse {
        generated_at: now,
        version: env!("CARGO_PKG_VERSION"),
        started_at: state.started_at,
        uptime_seconds: (now - state.started_at).num_seconds().max(0),
        server: ServerRuntime {
            memory_bytes: process_memory_bytes(),
            disk_used_percent: disk.as_ref().map(DiskUsage::used_percent),
            disk,
        },
        database,
        database_stats,
        object_storage,
        storage_bucket: state.minio.bucket_name().to_string(),
        bucket_stats,
        totals,
        purge: state.purge_status.get(),
        warnings,
    }))
}

/// Times a dependency check and turns a failure into something safe to show.
async fn measure<F, Fut>(check: F) -> DependencyHealth
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), AppError>>,
{
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(5), check()).await;
    let latency_ms = started.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(())) => DependencyHealth { reachable: true, latency_ms, detail: None },
        Ok(Err(error)) => {
            tracing::warn!(?error, "database health check failed");
            DependencyHealth {
                reachable: false,
                latency_ms,
                detail: Some("the database did not answer".to_string()),
            }
        }
        Err(_) => DependencyHealth {
            reachable: false,
            latency_ms,
            detail: Some("timed out after 5 seconds".to_string()),
        },
    }
}

async fn measure_storage(state: &AdminState) -> DependencyHealth {
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(5), state.minio.health_check()).await;
    let latency_ms = started.elapsed().as_millis() as u64;

    match result {
        Ok(Ok(())) => DependencyHealth { reachable: true, latency_ms, detail: None },
        Ok(Err(detail)) => DependencyHealth { reachable: false, latency_ms, detail: Some(detail) },
        Err(_) => DependencyHealth {
            reachable: false,
            latency_ms,
            detail: Some("timed out after 5 seconds".to_string()),
        },
    }
}

// ── Invites ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct AdminInviteSummary {
    id: String,
    code: String,
    label: Option<String>,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    used_at: Option<DateTime<Utc>>,
    used_by_username: Option<String>,
    status: &'static str,
}

#[derive(Serialize)]
struct AdminInvitesResponse {
    invites: Vec<AdminInviteSummary>,
    /// False while sign-ups are open, when a code is not needed to join.
    invites_required: bool,
    /// Where to send someone with a code.
    register_url: String,
}

async fn list_invites(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Result<Json<AdminInvitesResponse>, AppError> {
    authorize_admin(&state, &headers).await?;
    Ok(Json(build_invites(&state, &headers).await?))
}

async fn create_invite(
    State(state): State<AdminState>,
    headers: HeaderMap,
    Json(body): Json<CreateInviteRequest>,
) -> Result<Json<AdminInvitesResponse>, AppError> {
    authorize_admin(&state, &headers).await?;

    let label = normalize_optional(body.label);
    let expires_at = match body.expires_in_days {
        None => None,
        Some(days) if (1..=365).contains(&days) => Some(Utc::now() + ChronoDuration::days(days)),
        Some(_) => {
            return Err(AppError::BadRequest(
                "expires_in_days must be between 1 and 365, or left out for no expiry".to_string(),
            ))
        }
    };

    let invite = RegistrationInvite {
        id: Uuid::new_v4().to_string(),
        code: generate_invite_code(),
        label: label.clone(),
        created_at: Utc::now(),
        expires_at,
        used_at: None,
        used_by_username: None,
    };

    state.db.create_registration_invite(&invite).await?;

    write_audit_event(
        &state,
        make_audit_event(
            "invite",
            &invite.id,
            "invite_created",
            match label.as_deref() {
                Some(label) => format!("Created an invite for {label}"),
                None => "Created an invite".to_string(),
            },
            // Deliberately not the code itself: the audit trail is shown on a
            // page anyone with the admin token can read, and it outlives the
            // invite.
            Some(match expires_at {
                Some(at) => format!("Expires {at}"),
                None => "Does not expire".to_string(),
            }),
        ),
    )
    .await?;

    Ok(Json(build_invites(&state, &headers).await?))
}

async fn revoke_invite(
    State(state): State<AdminState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<AdminInvitesResponse>, AppError> {
    authorize_admin(&state, &headers).await?;

    if !state.db.delete_registration_invite(&id).await? {
        return Err(AppError::NotFound("invite not found".to_string()));
    }

    write_audit_event(
        &state,
        make_audit_event("invite", &id, "invite_revoked", "Revoked an invite".to_string(), None),
    )
    .await?;

    Ok(Json(build_invites(&state, &headers).await?))
}

async fn build_invites(
    state: &AdminState,
    headers: &HeaderMap,
) -> Result<AdminInvitesResponse, AppError> {
    let now = Utc::now();
    let invites = state
        .db
        .list_registration_invites()
        .await?
        .into_iter()
        .map(|invite| AdminInviteSummary {
            status: invite.status(now),
            id: invite.id,
            code: invite.code,
            label: invite.label,
            created_at: invite.created_at,
            expires_at: invite.expires_at,
            used_at: invite.used_at,
            used_by_username: invite.used_by_username,
        })
        .collect();

    Ok(AdminInvitesResponse {
        invites,
        invites_required: !state.settings.get().registration_enabled,
        register_url: format!("{}/register", public_base_url(headers)),
    })
}

/// Rebuilds this instance's own public address from the request, so the link
/// handed to an invited person points at the server they can actually reach
/// rather than at a hostname baked in at build time.
fn public_base_url(headers: &HeaderMap) -> String {
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("localhost:8090");

    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(if host.starts_with("localhost") || host.starts_with("127.0.0.1") {
            "http"
        } else {
            "https"
        });

    format!("{scheme}://{host}")
}

// ── Maintenance ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct PurgeRunResponse {
    cleared: usize,
    purge: PurgeStatus,
}

/// Runs the expired-share cleanup now instead of waiting for the daily sweep.
async fn run_share_purge(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Result<Json<PurgeRunResponse>, AppError> {
    authorize_admin(&state, &headers).await?;

    crate::purge_expired_shares(&state.db, &state.minio, &state.purge_status).await;
    let purge = state.purge_status.get();

    write_audit_event(
        &state,
        make_audit_event(
            "instance",
            "maintenance",
            "share_purge_run",
            format!("Ran the expired-share cleanup; cleared {}", purge.last_cleared),
            purge.last_error.clone(),
        ),
    )
    .await?;

    Ok(Json(PurgeRunResponse { cleared: purge.last_cleared, purge }))
}

async fn get_overview(
    State(state): State<AdminState>,
    headers: HeaderMap,
) -> Result<Json<AdminOverviewResponse>, AppError> {
    authorize_admin(&state, &headers).await?;
    Ok(Json(build_overview(&state).await?))
}

/// Instance settings, plus the context an operator needs to set them sensibly.
#[derive(Serialize)]
struct AdminSettingsResponse {
    settings: InstanceSettings,
    /// The address this very request was attributed to. An operator comparing
    /// it against their own public address can tell at a glance whether
    /// `trust_proxy_headers` is set the way their deployment needs — the
    /// throttles key on exactly this value.
    observed_client_address: String,
    /// Whether an `X-Forwarded-For` header was present on this request.
    forwarded_header_present: bool,
    /// The transport's own ceiling on a single request body. No attachment can
    /// exceed it however high the setting is raised.
    max_upload_body_bytes: i64,
}

async fn get_settings(
    State(state): State<AdminState>,
    headers: HeaderMap,
    client_ip: ClientIp,
) -> Result<Json<AdminSettingsResponse>, AppError> {
    authorize_admin(&state, &headers).await?;
    Ok(Json(settings_response(&state, &headers, client_ip)))
}

async fn update_settings(
    State(state): State<AdminState>,
    headers: HeaderMap,
    client_ip: ClientIp,
    Json(body): Json<UpdateInstanceSettingsRequest>,
) -> Result<Json<AdminSettingsResponse>, AppError> {
    authorize_admin(&state, &headers).await?;

    let current = state.settings.get();
    let next = body.apply(&current).map_err(AppError::BadRequest)?;
    let saved = state.db.save_instance_settings(&next).await?;
    state.settings.set(saved.clone());

    if let Some(detail) = describe_settings_change(&current, &saved) {
        write_audit_event(
            &state,
            make_audit_event(
                "instance",
                "settings",
                "instance_settings_updated",
                "Updated instance settings".to_string(),
                Some(detail),
            ),
        )
        .await?;
    }

    Ok(Json(settings_response(&state, &headers, client_ip)))
}

fn settings_response(
    state: &AdminState,
    headers: &HeaderMap,
    client_ip: ClientIp,
) -> AdminSettingsResponse {
    AdminSettingsResponse {
        settings: state.settings.get(),
        observed_client_address: client_ip.to_string(),
        forwarded_header_present: headers.contains_key("x-forwarded-for"),
        max_upload_body_bytes: MAX_UPLOAD_BODY_BYTES as i64,
    }
}

/// Records what actually changed, so the audit trail says "registration
/// disabled" rather than "settings saved". Returns `None` when a save left
/// everything as it was.
fn describe_settings_change(before: &InstanceSettings, after: &InstanceSettings) -> Option<String> {
    let mut changes = Vec::new();
    let mut note = |field: &str, before: String, after: String| {
        if before != after {
            changes.push(format!("{field}: {before} → {after}"));
        }
    };

    note(
        "registration_enabled",
        before.registration_enabled.to_string(),
        after.registration_enabled.to_string(),
    );
    note(
        "user_storage_limit_bytes",
        before.user_storage_limit_bytes.to_string(),
        after.user_storage_limit_bytes.to_string(),
    );
    note(
        "max_attachment_size_bytes",
        before.max_attachment_size_bytes.to_string(),
        after.max_attachment_size_bytes.to_string(),
    );
    note(
        "auth_rate_limit_per_minute",
        before.auth_rate_limit_per_minute.to_string(),
        after.auth_rate_limit_per_minute.to_string(),
    );
    note(
        "failed_login_threshold",
        before.failed_login_threshold.to_string(),
        after.failed_login_threshold.to_string(),
    );
    note(
        "failed_login_lockout_minutes",
        before.failed_login_lockout_minutes.to_string(),
        after.failed_login_lockout_minutes.to_string(),
    );
    note(
        "trust_proxy_headers",
        before.trust_proxy_headers.to_string(),
        after.trust_proxy_headers.to_string(),
    );

    (!changes.is_empty()).then(|| changes.join("; "))
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
        locked_users: users.iter().filter(|user| user.is_locked).count(),
        total_vaults,
        total_used_bytes,
    };

    let storage_limit = state.settings.get().storage_limit();

    Ok(AdminOverviewResponse {
        generated_at: now,
        metrics,
        users: users
            .into_iter()
            .zip(storage.into_iter())
            .map(|(user, storage)| map_admin_user(user, storage, storage_limit))
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

    // Compared in constant time so the response latency does not leak how much
    // of the token a caller has guessed right. The lengths are checked first
    // and separately — `ct_eq` needs equal-length inputs, and the length of an
    // admin token is not the secret.
    let matches = provided.len() == expected.len()
        && bool::from(provided.as_bytes().ct_eq(expected.as_bytes()));
    if !matches {
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

fn map_admin_user(
    user: AdminUserRecord,
    storage: UserStorageSummary,
    storage_limit_bytes: Option<i64>,
) -> AdminUserSummary {
    AdminUserSummary {
        id: user.id,
        username: user.username,
        created_at: user.created_at,
        first_name: user.first_name,
        last_name: user.last_name,
        email: user.email,
        is_locked: user.is_locked,
        locked_reason: user.locked_reason,
        admin_note: user.admin_note,
        vault_count: storage.vault_count,
        used_bytes: storage.used_bytes,
        storage_limit_bytes,
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