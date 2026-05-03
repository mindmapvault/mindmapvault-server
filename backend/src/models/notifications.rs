use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPriority {
    Low,
    Medium,
    High,
}

impl NotificationPriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "high" => Self::High,
            "low" => Self::Low,
            _ => Self::Medium,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserNotificationSettings {
    pub inbox_enabled: bool,
    pub email_enabled: bool,
    pub push_enabled: bool,
    pub desktop_enabled: bool,
    pub digest_enabled: bool,
    pub quiet_hours_start: Option<String>,
    pub quiet_hours_end: Option<String>,
    pub allow_preview_local_only: bool,
    pub share_created: bool,
    pub share_revoked: bool,
    pub attachment_upload_failures: bool,
    pub billing_notices: bool,
    pub security_alerts: bool,
    pub admin_messages: bool,
    pub collaboration_mentions: bool,
    pub updated_at: DateTime<Utc>,
}

impl Default for UserNotificationSettings {
    fn default() -> Self {
        Self {
            inbox_enabled: true,
            email_enabled: false,
            push_enabled: false,
            desktop_enabled: true,
            digest_enabled: false,
            quiet_hours_start: None,
            quiet_hours_end: None,
            allow_preview_local_only: true,
            share_created: true,
            share_revoked: true,
            attachment_upload_failures: true,
            billing_notices: true,
            security_alerts: true,
            admin_messages: true,
            collaboration_mentions: false,
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserNotificationSettingsRequest {
    pub inbox_enabled: Option<bool>,
    pub email_enabled: Option<bool>,
    pub push_enabled: Option<bool>,
    pub desktop_enabled: Option<bool>,
    pub digest_enabled: Option<bool>,
    pub quiet_hours_start: Option<Option<String>>,
    pub quiet_hours_end: Option<Option<String>>,
    pub allow_preview_local_only: Option<bool>,
    pub share_created: Option<bool>,
    pub share_revoked: Option<bool>,
    pub attachment_upload_failures: Option<bool>,
    pub billing_notices: Option<bool>,
    pub security_alerts: Option<bool>,
    pub admin_messages: Option<bool>,
    pub collaboration_mentions: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct StoredNotificationEvent {
    pub id: String,
    pub user_id: String,
    pub event_type: String,
    pub category: String,
    pub priority: NotificationPriority,
    pub actor_user_id: Option<String>,
    pub object_type: String,
    pub object_id: String,
    pub object_label_safe: Option<String>,
    pub reason_code: String,
    pub payload_json: Value,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    pub saved_at: Option<DateTime<Utc>>,
    pub done_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewNotificationEvent {
    pub id: String,
    pub user_id: String,
    pub event_type: String,
    pub category: String,
    pub priority: NotificationPriority,
    pub actor_user_id: Option<String>,
    pub object_type: String,
    pub object_id: String,
    pub object_label_safe: Option<String>,
    pub reason_code: String,
    pub payload_json: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct NotificationEventResponse {
    pub id: String,
    pub event_type: String,
    pub category: String,
    pub priority: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_user_id: Option<String>,
    pub object_type: String,
    pub object_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_label_safe: Option<String>,
    pub reason_code: String,
    pub payload_json: Value,
    pub created_at: DateTime<Utc>,
    pub unread: bool,
    pub saved: bool,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_at: Option<DateTime<Utc>>,
}

impl From<StoredNotificationEvent> for NotificationEventResponse {
    fn from(value: StoredNotificationEvent) -> Self {
        Self {
            id: value.id,
            event_type: value.event_type,
            category: value.category,
            priority: value.priority.as_str().to_string(),
            actor_user_id: value.actor_user_id,
            object_type: value.object_type,
            object_id: value.object_id,
            object_label_safe: value.object_label_safe,
            reason_code: value.reason_code,
            payload_json: value.payload_json,
            created_at: value.created_at,
            unread: value.read_at.is_none(),
            saved: value.saved_at.is_some(),
            done: value.done_at.is_some(),
            read_at: value.read_at,
            saved_at: value.saved_at,
            done_at: value.done_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateNotificationStateRequest {
    pub value: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct NotificationListQuery {
    pub category: Option<String>,
    pub state: Option<String>,
    pub limit: Option<usize>,
}