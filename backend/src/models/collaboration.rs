use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::plaintext_map::PlainTextAccessRole;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationPresence {
    pub session_id: String,
    pub user_id: String,
    pub username: String,
    pub role: PlainTextAccessRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_node_id: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationSnapshotPayload {
    pub map_id: String,
    pub seq: u64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub content_json: Value,
    pub participants: Vec<CollaborationPresence>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_user_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationSnapshotResponse {
    pub snapshot: CollaborationSnapshotPayload,
    pub access_role: PlainTextAccessRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationEvent {
    pub seq: u64,
    pub map_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub content_json: Value,
    pub actor_user_id: String,
    pub actor_username: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationOpsResponse {
    pub current_seq: u64,
    pub events: Vec<CollaborationEvent>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CollaborationClientMessage {
    Hello {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        _client_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        _last_seen_seq: Option<u64>,
    },
    DocumentUpdate {
        base_seq: u64,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        content_json: Value,
    },
    PresenceUpdate {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selected_node_id: Option<String>,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CollaborationServerMessage {
    HelloAck {
        session_id: String,
        role: PlainTextAccessRole,
        seq: u64,
    },
    Snapshot {
        snapshot: CollaborationSnapshotPayload,
    },
    PresenceJoined {
        presence: CollaborationPresence,
    },
    PresenceUpdated {
        presence: CollaborationPresence,
    },
    PresenceLeft {
        session_id: String,
    },
    ResyncRequired {
        seq: u64,
        reason: String,
    },
    Error {
        message: String,
    },
    Pong,
}