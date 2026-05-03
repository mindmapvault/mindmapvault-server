use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShareStatus {
    Pending,
    Available,
    Revoked,
}

impl ShareStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Available => "available",
            Self::Revoked => "revoked",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "available" => Self::Available,
            "revoked" => Self::Revoked,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShareScope {
    Map,
    Node,
    Note,
}

impl ShareScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Node => "node",
            Self::Note => "note",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "node" => Self::Node,
            "note" => Self::Note,
            _ => Self::Map,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateMapShareRequest {
    pub name: String,
    pub scope: ShareScope,
    pub include_attachments: bool,
    pub passphrase_hint: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub content_type: String,
    pub size_bytes: i64,
    pub encryption_meta: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct CreateMapShareResponse {
    pub share_id: String,
    pub share_url: String,
    pub s3_key: String,
    pub upload_url: String,
    pub upload_headers: BTreeMap<String, String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteMapShareUploadRequest {
    pub version_id: String,
    pub checksum_sha256: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MapShareOwnerSummary {
    pub id: String,
    pub map_id: String,
    pub name: String,
    pub scope: ShareScope,
    pub share_url: String,
    pub include_attachments: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: ShareStatus,
    pub content_type: String,
    pub size_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MapShareAttachmentMetadata {
    pub id: String,
    pub share_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub name: String,
    pub sanitized_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub uploaded_at: DateTime<Utc>,
    pub encryption_meta: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum_sha256: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PublicMapShareResponse {
    pub id: String,
    pub name: String,
    pub scope: ShareScope,
    pub include_attachments: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passphrase_hint: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub content_type: String,
    pub size_bytes: i64,
    pub encryption_meta: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum_sha256: Option<String>,
    pub download_url: String,
    pub download_expires_at: DateTime<Utc>,
    pub attachments: Vec<MapShareAttachmentMetadata>,
}

#[derive(Debug, Deserialize)]
pub struct InitMapShareAttachmentRequest {
    pub name: String,
    pub content_type: String,
    pub size: i64,
    pub node_id: Option<String>,
    pub source_attachment_id: Option<String>,
    pub encryption_meta: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct InitMapShareAttachmentResponse {
    pub attachment_id: String,
    pub s3_key: String,
    pub upload_url: String,
    pub upload_headers: BTreeMap<String, String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteMapShareAttachmentUploadRequest {
    pub version_id: String,
    pub checksum_sha256: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MapShareAttachmentDownloadResponse {
    pub download_url: String,
    pub expires_at: DateTime<Utc>,
    pub content_type: String,
    pub name: String,
    pub sanitized_name: String,
    pub size_bytes: i64,
    pub encryption_meta: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum_sha256: Option<String>,
}