use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Per-version KEM envelope appended to the document on every confirmed upload.
/// This allows the client to recover the DEK that was used when that specific
/// MinIO version was saved, so historical blobs can later be decrypted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionSnapshot {
    pub version_id: String,
    pub eph_classical_public: String,
    pub eph_pq_ciphertext: String,
    pub wrapped_dek: String,
    pub saved_at: DateTime<Utc>,
}
// ── DTOs ──────────────────────────────────────────────────────────────────────

/// Sent by the client when creating or updating a mind map.
#[derive(Debug, Deserialize)]
pub struct UpsertMindMapRequest {
    pub title_encrypted: String,
    pub eph_classical_public: String,
    pub eph_pq_ciphertext: String,
    pub wrapped_dek: String,
}

/// Update just the vault-level display metadata (color, note, max_versions, title).
#[derive(Debug, Deserialize)]
pub struct UpdateVaultMetaRequest {
    pub vault_color: Option<String>,
    pub vault_note_encrypted: Option<String>,
    pub vault_encryption_mode: Option<String>,
    pub max_versions: Option<u32>,
    /// Re-encrypted vault title (optional — only sent on rename).
    pub title_encrypted: Option<String>,
    /// Vault-level labels/tags for this vault (user-only, not shared).
    pub vault_labels: Option<Vec<String>>,
}

/// Lightweight list item — no key material, client decrypts title itself.
#[derive(Debug, Serialize)]
pub struct MindMapListItem {
    pub id: String,
    pub title_encrypted: String,
    pub vault_color: Option<String>,
    pub vault_note_encrypted: Option<String>,
    pub vault_encryption_mode: String,
    pub max_versions: u32,
    pub vault_labels: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Full metadata returned for a single mind map (no ciphertext body — that
/// comes via a presigned MinIO URL).
#[derive(Debug, Serialize)]
pub struct MindMapDetail {
    pub id: String,
    pub title_encrypted: String,
    pub eph_classical_public: String,
    pub eph_pq_ciphertext: String,
    pub wrapped_dek: String,
    pub vault_color: Option<String>,
    pub vault_note_encrypted: Option<String>,
    pub vault_encryption_mode: String,
    pub max_versions: u32,
    pub total_version_count: usize,
    /// Latest confirmed version in MinIO (None until first confirm-upload).
    pub minio_version_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Per-vault storage summary returned by GET /api/mindmaps/storage.
#[derive(Debug, Serialize)]
pub struct VaultStorageInfo {
    pub id: String,
    pub title_encrypted: String,
    pub version_count: usize,
    pub attachment_count: usize,
    pub attachment_bytes: i64,
    pub total_bytes: i64,
}

/// Total storage summary for the authenticated user.
#[derive(Debug, Serialize)]
pub struct StorageSummary {
    pub vaults: Vec<VaultStorageInfo>,
    pub total_bytes: i64,
    pub attachment_count: usize,
    pub attachment_bytes: i64,
    /// Backward-compatible field used by existing clients as the active cloud limit.
    pub free_tier_bytes: i64,
    /// Current plan tier string ("free" | "paid").
    pub plan_tier: String,
    /// Active plan storage limit in bytes.
    pub plan_limit_bytes: i64,
}

/// Returned after a successful create/update so the client knows the object key.
#[derive(Debug, Serialize)]
pub struct MindMapCreatedResponse {
    pub id: String,
    pub minio_object_key: String,
    pub upload_url: String,
}

/// Returned for download.
#[derive(Debug, Serialize)]
pub struct PresignedUrlResponse {
    pub url: String,
    pub expires_in_secs: u64,
    /// The version this URL points to (None = latest).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
}

/// Sent by the client after a successful direct upload to MinIO.
/// The `version_id` comes from the `x-amz-version-id` response header.
#[derive(Debug, Deserialize)]
pub struct ConfirmUploadRequest {
    pub version_id: String,
}

/// Response after confirming an upload.
#[derive(Debug, Serialize)]
pub struct ConfirmUploadResponse {
    pub version_id: String,
}

/// Single entry returned by `GET /:id/versions`.
/// Combines MinIO storage metadata with the KEM envelope needed to decrypt
/// that version's blob (present for all versions saved after history tracking
/// was introduced; absent for very old versions).
#[derive(Debug, Serialize)]
pub struct VersionDetail {
    pub version_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_number: Option<usize>,
    pub is_latest: bool,
    pub last_modified: DateTime<Utc>,
    /// Encrypted blob size in bytes.
    pub size_bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eph_classical_public: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eph_pq_ciphertext: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrapped_dek: Option<String>,
    /// When this version was saved (from the snapshot; None for legacy versions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<DateTime<Utc>>,
}

