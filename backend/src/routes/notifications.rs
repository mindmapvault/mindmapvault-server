use std::sync::Arc;

use axum::{
    extract::{FromRef, Path, Query, State},
    routing::{get, patch, post},
    Json, Router,
};
use chrono::Utc;

use crate::{
    db::sql_store::DynSqlStore,
    error::AppError,
    middleware::auth::{AuthenticatedUser, JwtService},
    models::notifications::{
        NotificationEventResponse, NotificationListQuery, UpdateNotificationStateRequest,
        UpdateUserNotificationSettingsRequest, UserNotificationSettings,
    },
};

#[derive(Clone)]
pub struct NotificationsState {
    pub db: DynSqlStore,
    pub jwt: Arc<JwtService>,
}

impl FromRef<NotificationsState> for Arc<JwtService> {
    fn from_ref(state: &NotificationsState) -> Self {
        state.jwt.clone()
    }
}

pub fn router(state: NotificationsState) -> Router {
    Router::new()
        .route("/", get(list_notifications))
        .route("/mark-all-read", post(mark_all_read))
        .route("/settings", get(get_notification_settings).patch(update_notification_settings))
        .route("/{notification_id}/read", patch(mark_notification_read))
        .route("/{notification_id}/saved", patch(mark_notification_saved))
        .route("/{notification_id}/done", patch(mark_notification_done))
        .with_state(state)
}

async fn list_notifications(
    State(state): State<NotificationsState>,
    user: AuthenticatedUser,
    Query(query): Query<NotificationListQuery>,
) -> Result<Json<Vec<NotificationEventResponse>>, AppError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let events = state
        .db
        .list_notification_events(
            &user.0,
            query.category.as_deref(),
            query.state.as_deref(),
            limit,
        )
        .await?;

    Ok(Json(events.into_iter().map(NotificationEventResponse::from).collect()))
}

async fn get_notification_settings(
    State(state): State<NotificationsState>,
    user: AuthenticatedUser,
) -> Result<Json<UserNotificationSettings>, AppError> {
    let settings = state
        .db
        .load_user_notification_settings(&user.0)
        .await?
        .unwrap_or_default();

    Ok(Json(settings))
}

async fn update_notification_settings(
    State(state): State<NotificationsState>,
    user: AuthenticatedUser,
    Json(body): Json<UpdateUserNotificationSettingsRequest>,
) -> Result<Json<UserNotificationSettings>, AppError> {
    let mut settings = state
        .db
        .load_user_notification_settings(&user.0)
        .await?
        .unwrap_or_default();

    if let Some(value) = body.inbox_enabled {
        settings.inbox_enabled = value;
    }
    if let Some(value) = body.email_enabled {
        settings.email_enabled = value;
    }
    if let Some(value) = body.push_enabled {
        settings.push_enabled = value;
    }
    if let Some(value) = body.desktop_enabled {
        settings.desktop_enabled = value;
    }
    if let Some(value) = body.digest_enabled {
        settings.digest_enabled = value;
    }
    if let Some(value) = body.quiet_hours_start {
        settings.quiet_hours_start = normalize_optional_time(value)?;
    }
    if let Some(value) = body.quiet_hours_end {
        settings.quiet_hours_end = normalize_optional_time(value)?;
    }
    if let Some(value) = body.allow_preview_local_only {
        settings.allow_preview_local_only = value;
    }
    if let Some(value) = body.share_created {
        settings.share_created = value;
    }
    if let Some(value) = body.share_revoked {
        settings.share_revoked = value;
    }
    if let Some(value) = body.attachment_upload_failures {
        settings.attachment_upload_failures = value;
    }
    if let Some(value) = body.billing_notices {
        settings.billing_notices = value;
    }
    if let Some(value) = body.security_alerts {
        settings.security_alerts = value;
    }
    if let Some(value) = body.admin_messages {
        settings.admin_messages = value;
    }
    if let Some(value) = body.collaboration_mentions {
        settings.collaboration_mentions = value;
    }
    settings.updated_at = Utc::now();

    state
        .db
        .upsert_user_notification_settings(&user.0, settings.clone())
        .await?;

    Ok(Json(settings))
}

async fn mark_notification_read(
    State(state): State<NotificationsState>,
    user: AuthenticatedUser,
    Path(notification_id): Path<String>,
    Json(body): Json<UpdateNotificationStateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    update_state(
        state,
        user,
        notification_id,
        body.value.unwrap_or(true),
        NotificationStateAction::Read,
    )
    .await
}

async fn mark_notification_saved(
    State(state): State<NotificationsState>,
    user: AuthenticatedUser,
    Path(notification_id): Path<String>,
    Json(body): Json<UpdateNotificationStateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    update_state(
        state,
        user,
        notification_id,
        body.value.unwrap_or(true),
        NotificationStateAction::Saved,
    )
    .await
}

async fn mark_notification_done(
    State(state): State<NotificationsState>,
    user: AuthenticatedUser,
    Path(notification_id): Path<String>,
    Json(body): Json<UpdateNotificationStateRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    update_state(
        state,
        user,
        notification_id,
        body.value.unwrap_or(true),
        NotificationStateAction::Done,
    )
    .await
}

async fn mark_all_read(
    State(state): State<NotificationsState>,
    user: AuthenticatedUser,
) -> Result<Json<serde_json::Value>, AppError> {
    let updated = state.db.mark_all_notifications_read(&user.0).await?;
    Ok(Json(serde_json::json!({ "updated": updated })))
}

enum NotificationStateAction {
    Read,
    Saved,
    Done,
}

async fn update_state(
    state: NotificationsState,
    user: AuthenticatedUser,
    notification_id: String,
    value: bool,
    action: NotificationStateAction,
) -> Result<Json<serde_json::Value>, AppError> {
    let updated = match action {
        NotificationStateAction::Read => {
            state
                .db
                .mark_notification_read(&user.0, &notification_id, value)
                .await?
        }
        NotificationStateAction::Saved => {
            state
                .db
                .mark_notification_saved(&user.0, &notification_id, value)
                .await?
        }
        NotificationStateAction::Done => {
            state
                .db
                .mark_notification_done(&user.0, &notification_id, value)
                .await?
        }
    };

    if !updated {
        return Err(AppError::NotFound("notification not found".to_string()));
    }

    Ok(Json(serde_json::json!({ "updated": true })))
}

fn normalize_optional_time(value: Option<String>) -> Result<Option<String>, AppError> {
    value.map(validate_time_of_day).transpose()
}

fn validate_time_of_day(value: String) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("quiet hour values cannot be empty strings".to_string()));
    }
    let parts: Vec<_> = trimmed.split(':').collect();
    if parts.len() != 2 {
        return Err(AppError::BadRequest("quiet hour values must use HH:MM format".to_string()));
    }
    let hours = parts[0]
        .parse::<u32>()
        .map_err(|_| AppError::BadRequest("quiet hour values must use HH:MM format".to_string()))?;
    let minutes = parts[1]
        .parse::<u32>()
        .map_err(|_| AppError::BadRequest("quiet hour values must use HH:MM format".to_string()))?;
    if hours > 23 || minutes > 59 {
        return Err(AppError::BadRequest("quiet hour values must use HH:MM format".to_string()));
    }
    Ok(format!("{hours:02}:{minutes:02}"))
}