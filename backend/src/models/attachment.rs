use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentStatus {
    Pending,
    Available,
    Deleted,
}

impl AttachmentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Available => "available",
            Self::Deleted => "deleted",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "available" => Self::Available,
            "deleted" => Self::Deleted,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct InitAttachmentRequest {
    pub name: String,
    pub content_type: String,
    pub size: i64,
    pub node_id: Option<String>,
    pub encrypted: bool,
    pub encryption_meta: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct InitAttachmentResponse {
    pub attachment_id: String,
    pub s3_key: String,
    pub upload_url: String,
    pub upload_headers: BTreeMap<String, String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteAttachmentUploadRequest {
    pub version_id: String,
    pub checksum_sha256: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAttachmentRequest {
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentMetadata {
    pub id: String,
    pub map_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub name: String,
    pub sanitized_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub uploaded_by: String,
    pub uploaded_at: DateTime<Utc>,
    pub encrypted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_meta: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s3_version_id: Option<String>,
    pub status: AttachmentStatus,
}

#[derive(Debug, Serialize)]
pub struct AttachmentDownloadResponse {
    pub download_url: String,
    pub expires_at: DateTime<Utc>,
    pub encrypted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_meta: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    pub content_type: String,
    pub name: String,
    pub sanitized_name: String,
    pub size_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum_sha256: Option<String>,
}