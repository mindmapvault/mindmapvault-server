use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One saved version of a vault: where its ciphertext lives, and the KEM
/// envelope that unwraps the DEK it was sealed with.
///
/// This row is the record of a version's existence. The object store is asked
/// only to hand back the bytes at `object_key`; it is never asked which
/// versions exist or which one an id refers to, because that answer is not
/// portable across S3 implementations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionSnapshot {
    pub version_id: String,
    pub eph_classical_public: String,
    pub eph_pq_ciphertext: String,
    pub wrapped_dek: String,
    pub saved_at: DateTime<Utc>,
    /// Key the ciphertext is stored under.
    ///
    /// `None` on rows written before per-version keys, whose bytes were an S3
    /// version of the base key. Those are recoverable only where the store
    /// really implemented versioning, so the migration resolves them once and
    /// nothing else has to carry the ambiguity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_key: Option<String>,
    /// Ciphertext size, recorded when the version was written.
    ///
    /// Kept here so listing versions never has to ask the store, which is what
    /// used to make every row read 0 B when the metadata call failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
}
/// Total ciphertext held by a vault's saved versions.
///
/// Sizes are recorded when each version is written, so this needs no call to
/// the object store. The listing that used to supply them is not portable, and
/// where it failed every version was reported as 0 B.
pub fn stored_version_bytes(version_history: &[VersionSnapshot]) -> i64 {
    version_history
        .iter()
        .filter_map(|snapshot| snapshot.size_bytes)
        .sum()
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
/// comes via a presigned the object store URL).
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
    /// Latest confirmed version in the object store (None until first confirm-upload).
    ///
    /// The JSON name keeps the old spelling on purpose: released desktop
    /// builds bundle their own frontend and read this field, so renaming it on
    /// the wire would break a new server talking to an older client.
    #[serde(rename = "minio_version_id")]
    pub current_version_id: Option<String>,
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
    /// Old JSON name kept for the same reason as `MindMapDetail`.
    #[serde(rename = "minio_object_key")]
    pub object_key: String,
    pub upload_url: String,
    /// The version the upload URL points at. The client reports this back to
    /// `confirm-upload` rather than an `x-amz-version-id` response header,
    /// which R2 does not send and Garage sends without honouring.
    pub version_id: String,
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

/// Sent by the client after a successful direct upload to the object store.
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
/// Combines the object store storage metadata with the KEM envelope needed to decrypt
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

