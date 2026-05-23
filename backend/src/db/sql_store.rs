use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::{
    error::AppError,
    models::{
        access::UserAccessGrant,
        admin_audit::AdminAuditEvent,
        attachment::AttachmentStatus,
        mindmap::VersionSnapshot,
        settings::UserAccountSettings,
        user::{Argon2Params, SubscriptionTier},
    },
};

#[derive(Debug, Clone)]
pub struct StoredUser {
    pub id: String,
    pub username: String,
    pub auth_hash: String,
    pub argon2_salt: String,
    pub argon2_params: Argon2Params,
    pub classical_public_key: String,
    pub pq_public_key: String,
    pub classical_priv_encrypted: String,
    pub pq_priv_encrypted: String,
    pub key_version: u32,
    pub created_at: DateTime<Utc>,
    pub subscription_tier: SubscriptionTier,
    pub stripe_customer_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
    pub stripe_subscription_status: Option<String>,
    pub subscription_current_period_end: Option<DateTime<Utc>>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub is_locked: bool,
    pub locked_reason: Option<String>,
    pub admin_note: Option<String>,
    pub manual_subscription_tier: Option<SubscriptionTier>,
    pub manual_subscription_expires_at: Option<DateTime<Utc>>,
    pub manual_subscription_reason: Option<String>,
    pub manual_subscription_granted_by: Option<String>,
    pub access_grants: Vec<UserAccessGrant>,
}

#[derive(Debug, Clone)]
pub struct NewUser {
    pub id: String,
    pub username: String,
    pub auth_hash: String,
    pub argon2_salt: String,
    pub argon2_params: Argon2Params,
    pub classical_public_key: String,
    pub pq_public_key: String,
    pub classical_priv_encrypted: String,
    pub pq_priv_encrypted: String,
    pub key_version: u32,
    pub created_at: DateTime<Utc>,
    pub subscription_tier: SubscriptionTier,
    pub stripe_customer_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
    pub stripe_subscription_status: Option<String>,
    pub subscription_current_period_end: Option<DateTime<Utc>>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub is_locked: bool,
    pub locked_reason: Option<String>,
    pub admin_note: Option<String>,
    pub manual_subscription_tier: Option<SubscriptionTier>,
    pub manual_subscription_expires_at: Option<DateTime<Utc>>,
    pub manual_subscription_reason: Option<String>,
    pub manual_subscription_granted_by: Option<String>,
    pub access_grants: Vec<UserAccessGrant>,
}

#[derive(Debug, Clone)]
pub struct UserProfileUpdate {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
}

/// All credential and vault-title changes committed during a password rotation.
/// The backend hashes `new_auth_token` before writing — the raw token never rests
/// on disk server-side.
#[derive(Debug, Clone)]
pub struct RotateCredentialsUpdate {
    /// HKDF(new_master_key, "crypt-mind-auth-v1") hex — the server hashes this.
    pub new_auth_token: String,
    pub new_argon2_salt: String,
    pub new_argon2_params: Argon2Params,
    pub new_classical_priv_encrypted: String,
    pub new_pq_priv_encrypted: String,
    pub new_key_version: u32,
    pub updated_vaults: Vec<RotateVaultEntry>,
}

#[derive(Debug, Clone)]
pub struct RotateVaultEntry {
    pub id: String,
    pub title_encrypted: String,
    /// `None` → preserve existing note; `Some("")` → clear note; `Some(ct)` → update.
    pub vault_note_encrypted: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AdminUserRecord {
    pub id: String,
    pub username: String,
    pub created_at: DateTime<Utc>,
    pub subscription_tier: SubscriptionTier,
    pub stripe_customer_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
    pub stripe_subscription_status: Option<String>,
    pub subscription_current_period_end: Option<DateTime<Utc>>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub is_locked: bool,
    pub locked_reason: Option<String>,
    pub admin_note: Option<String>,
    pub manual_subscription_tier: Option<SubscriptionTier>,
    pub manual_subscription_expires_at: Option<DateTime<Utc>>,
    pub manual_subscription_reason: Option<String>,
    pub manual_subscription_granted_by: Option<String>,
    pub access_grants: Vec<UserAccessGrant>,
}

#[derive(Debug, Clone)]
pub struct AdminUserAdminUpdate {
    pub admin_note: Option<String>,
    pub locked_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ManualSubscriptionUpdate {
    pub manual_subscription_tier: Option<SubscriptionTier>,
    pub manual_subscription_expires_at: Option<DateTime<Utc>>,
    pub manual_subscription_reason: Option<String>,
    pub manual_subscription_granted_by: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredMindMap {
    pub id: String,
    pub user_id: String,
    pub title_encrypted: String,
    pub minio_object_key: String,
    pub eph_classical_public: String,
    pub eph_pq_ciphertext: String,
    pub wrapped_dek: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub minio_version_id: Option<String>,
    pub version_history: Vec<VersionSnapshot>,
    pub vault_color: Option<String>,
    pub vault_note_encrypted: Option<String>,
    pub vault_encryption_mode: String,
    pub max_versions: u32,
    pub vault_labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NewMindMap {
    pub id: String,
    pub user_id: String,
    pub title_encrypted: String,
    pub minio_object_key: String,
    pub eph_classical_public: String,
    pub eph_pq_ciphertext: String,
    pub wrapped_dek: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub minio_version_id: Option<String>,
    pub version_history: Vec<VersionSnapshot>,
    pub vault_color: Option<String>,
    pub vault_note_encrypted: Option<String>,
    pub vault_encryption_mode: String,
    pub max_versions: u32,
    pub vault_labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MindMapContentUpdate {
    pub title_encrypted: String,
    pub eph_classical_public: String,
    pub eph_pq_ciphertext: String,
    pub wrapped_dek: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct MindMapMetaUpdate {
    pub title_encrypted: String,
    pub vault_color: Option<String>,
    pub vault_note_encrypted: Option<String>,
    pub vault_encryption_mode: String,
    pub max_versions: u32,
    pub vault_labels: Vec<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StoredMindMapAttachment {
    pub id: String,
    pub map_id: String,
    pub node_id: Option<String>,
    pub name: String,
    pub sanitized_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub s3_key: String,
    pub s3_version_id: Option<String>,
    pub uploaded_by: String,
    pub uploaded_at: DateTime<Utc>,
    pub encrypted: bool,
    pub encryption_meta: Option<Value>,
    pub checksum_sha256: Option<String>,
    pub status: AttachmentStatus,
}

#[derive(Debug, Clone)]
pub struct NewMindMapAttachment {
    pub id: String,
    pub map_id: String,
    pub node_id: Option<String>,
    pub name: String,
    pub sanitized_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub s3_key: String,
    pub s3_version_id: Option<String>,
    pub uploaded_by: String,
    pub uploaded_at: DateTime<Utc>,
    pub encrypted: bool,
    pub encryption_meta: Option<Value>,
    pub checksum_sha256: Option<String>,
    pub status: AttachmentStatus,
}

#[derive(Debug, Clone)]
pub struct MindMapAttachmentUploadUpdate {
    pub s3_version_id: String,
    pub checksum_sha256: Option<String>,
    pub status: AttachmentStatus,
}

impl StoredUser {
    pub fn manual_subscription_active(&self, now: DateTime<Utc>) -> bool {
        self.manual_subscription_tier.is_some()
            && self
                .manual_subscription_expires_at
                .map(|expires_at| expires_at > now)
                .unwrap_or(true)
    }

    pub fn effective_subscription_tier(&self, now: DateTime<Utc>) -> SubscriptionTier {
        if self.manual_subscription_active(now) {
            return self.manual_subscription_tier.clone().unwrap_or_default();
        }

        self.subscription_tier.clone()
    }

    pub fn effective_plan_source(&self, now: DateTime<Utc>) -> &'static str {
        if self.manual_subscription_active(now) {
            return "admin_override";
        }

        if matches!(
            self.stripe_subscription_status.as_deref(),
            Some("active") | Some("trialing") | Some("past_due")
        ) || self.stripe_customer_id.is_some()
        {
            return "stripe";
        }

        "base"
    }

    pub fn effective_access_grants(&self, now: DateTime<Utc>) -> Vec<UserAccessGrant> {
        crate::models::user::effective_access_grants_from_legacy(
            &self.access_grants,
            self.effective_subscription_tier(now),
            self.effective_plan_source(now),
            self.created_at,
            self.subscription_current_period_end,
            now,
        )
    }
}

impl AdminUserRecord {
    pub fn manual_subscription_active(&self, now: DateTime<Utc>) -> bool {
        self.manual_subscription_tier.is_some()
            && self
                .manual_subscription_expires_at
                .map(|expires_at| expires_at > now)
                .unwrap_or(true)
    }

    pub fn effective_subscription_tier(&self, now: DateTime<Utc>) -> SubscriptionTier {
        if self.manual_subscription_active(now) {
            return self.manual_subscription_tier.clone().unwrap_or_default();
        }

        self.subscription_tier.clone()
    }

    pub fn effective_plan_source(&self, now: DateTime<Utc>) -> &'static str {
        if self.manual_subscription_active(now) {
            return "admin_override";
        }

        if matches!(
            self.stripe_subscription_status.as_deref(),
            Some("active") | Some("trialing") | Some("past_due")
        ) || self.stripe_customer_id.is_some()
        {
            return "stripe";
        }

        "base"
    }

    pub fn effective_access_grants(&self, now: DateTime<Utc>) -> Vec<UserAccessGrant> {
        crate::models::user::effective_access_grants_from_legacy(
            &self.access_grants,
            self.effective_subscription_tier(now),
            self.effective_plan_source(now),
            self.created_at,
            self.subscription_current_period_end,
            now,
        )
    }
}

#[async_trait]
pub trait SqlStore: Send + Sync {
    async fn list_admin_audit_events(&self, limit: usize) -> Result<Vec<AdminAuditEvent>, AppError>;
    async fn create_admin_audit_event(&self, event: AdminAuditEvent) -> Result<(), AppError>;
    async fn list_admin_users(&self) -> Result<Vec<AdminUserRecord>, AppError>;
    async fn load_user_by_username(&self, username: &str) -> Result<Option<StoredUser>, AppError>;
    async fn load_user_by_id(&self, id: &str) -> Result<Option<StoredUser>, AppError>;
    async fn create_user(&self, user: NewUser) -> Result<(), AppError>;
    async fn update_user_profile(&self, user_id: &str, update: UserProfileUpdate)
        -> Result<(), AppError>;
    async fn rotate_user_credentials(
        &self,
        user_id: &str,
        update: RotateCredentialsUpdate,
    ) -> Result<(), AppError>;
    async fn update_user_stripe_customer_id(
        &self,
        user_id: &str,
        stripe_customer_id: &str,
    ) -> Result<(), AppError>;
    async fn update_user_subscription_by_customer_id(
        &self,
        stripe_customer_id: &str,
        subscription_tier: SubscriptionTier,
        stripe_subscription_status: Option<String>,
        subscription_current_period_end: Option<DateTime<Utc>>,
    ) -> Result<(), AppError>;
    async fn set_user_locked(&self, user_id: &str, is_locked: bool) -> Result<(), AppError>;
    async fn update_user_admin_fields(&self, user_id: &str, update: AdminUserAdminUpdate)
        -> Result<(), AppError>;
    async fn update_user_manual_subscription(
        &self,
        user_id: &str,
        update: ManualSubscriptionUpdate,
    ) -> Result<(), AppError>;
    async fn delete_user(&self, user_id: &str) -> Result<(), AppError>;
    async fn update_user_access_grants(
        &self,
        user_id: &str,
        access_grants: Vec<UserAccessGrant>,
    ) -> Result<(), AppError>;
    async fn load_user_account_settings(
        &self,
        user_id: &str,
    ) -> Result<Option<UserAccountSettings>, AppError>;
    async fn upsert_user_account_settings(
        &self,
        user_id: &str,
        settings: UserAccountSettings,
    ) -> Result<(), AppError>;

    async fn list_mind_maps(&self, user_id: &str) -> Result<Vec<StoredMindMap>, AppError>;
    async fn create_mind_map(&self, map: NewMindMap) -> Result<(), AppError>;
    async fn get_mind_map_owned(
        &self,
        id: &str,
        user_id: &str,
    ) -> Result<Option<StoredMindMap>, AppError>;
    async fn update_mind_map_content(
        &self,
        id: &str,
        user_id: &str,
        update: MindMapContentUpdate,
    ) -> Result<(), AppError>;
    async fn update_mind_map_upload(
        &self,
        id: &str,
        user_id: &str,
        minio_version_id: &str,
        version_history: Vec<VersionSnapshot>,
    ) -> Result<(), AppError>;
    async fn delete_mind_map(&self, id: &str, user_id: &str) -> Result<(), AppError>;
    async fn update_mind_map_meta(
        &self,
        id: &str,
        user_id: &str,
        update: MindMapMetaUpdate,
    ) -> Result<(), AppError>;
    async fn list_mind_map_attachments(
        &self,
        map_id: &str,
    ) -> Result<Vec<StoredMindMapAttachment>, AppError>;
    async fn create_mind_map_attachment(&self, attachment: NewMindMapAttachment)
        -> Result<(), AppError>;
    async fn get_mind_map_attachment(
        &self,
        map_id: &str,
        attachment_id: &str,
    ) -> Result<Option<StoredMindMapAttachment>, AppError>;
    async fn complete_mind_map_attachment_upload(
        &self,
        map_id: &str,
        attachment_id: &str,
        update: MindMapAttachmentUploadUpdate,
    ) -> Result<(), AppError>;
    async fn update_mind_map_attachment_node(
        &self,
        map_id: &str,
        attachment_id: &str,
        node_id: Option<String>,
    ) -> Result<(), AppError>;
    async fn mark_mind_map_attachment_deleted(
        &self,
        map_id: &str,
        attachment_id: &str,
    ) -> Result<(), AppError>;
}

pub type DynSqlStore = Arc<dyn SqlStore>;
