//! Turning a database row into a model.
//!
//! Shared by the query modules beside this one.

use tokio_postgres::{types::Json, Row};

use crate::{
    db::sql_store::{
        AdminUserRecord,
        StoredMindMap, StoredMindMapAttachment, StoredMindMapShare, StoredMindMapShareAttachment,
        StoredUser,
    },
    error::AppError,
    models::{
        access::UserAccessGrant,
        admin_audit::AdminAuditEvent,
        attachment::AttachmentStatus,
        invite::RegistrationInvite,
        mindmap::VersionSnapshot,
        settings::UserAccountSettings,
        share::{ShareScope, ShareStatus},
        user::{Argon2Params, SubscriptionTier},
    },
};

pub(super) fn stored_mind_map_share_from_row(row: Row) -> Result<StoredMindMapShare, AppError> {
    Ok(StoredMindMapShare {
        id: row.get("id"),
        map_id: row.get("map_id"),
        share_name: row.get("share_name"),
        scope: ShareScope::from_str(&row.get::<_, String>("share_scope")),
        s3_key: row.get("s3_key"),
        s3_version_id: row.get("s3_version_id"),
        created_by: row.get("created_by"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        expires_at: row.get("expires_at"),
        revoked: row.get("revoked"),
        include_attachments: row.get("include_attachments"),
        passphrase_hint: row.get("passphrase_hint"),
        content_type: row.get("content_type"),
        size_bytes: row.get("size_bytes"),
        encryption_meta: row.get::<_, Json<serde_json::Value>>("encryption_meta").0,
        checksum_sha256: row.get("checksum_sha256"),
        status: ShareStatus::from_str(&row.get::<_, String>("status")),
    })
}
pub(super) fn stored_mind_map_share_attachment_from_row(
    row: Row,
) -> Result<StoredMindMapShareAttachment, AppError> {
    Ok(StoredMindMapShareAttachment {
        id: row.get("id"),
        share_id: row.get("share_id"),
        source_attachment_id: row.get("source_attachment_id"),
        node_id: row.get("node_id"),
        name: row.get("name"),
        sanitized_name: row.get("sanitized_name"),
        content_type: row.get("content_type"),
        size_bytes: row.get("size_bytes"),
        s3_key: row.get("s3_key"),
        s3_version_id: row.get("s3_version_id"),
        uploaded_at: row.get("uploaded_at"),
        encryption_meta: row.get::<_, Json<serde_json::Value>>("encryption_meta").0,
        checksum_sha256: row.get("checksum_sha256"),
        status: AttachmentStatus::from_str(&row.get::<_, String>("status")),
    })
}

pub(super) fn stored_user_from_row(row: Row) -> Result<StoredUser, AppError> {
    Ok(StoredUser {
        id: row.get("id"),
        username: row.get("username"),
        auth_hash: row.get("auth_hash"),
        argon2_salt: row.get("argon2_salt"),
        argon2_params: row.get::<_, Json<Argon2Params>>("argon2_params").0,
        classical_public_key: row.get("classical_public_key"),
        pq_public_key: row.get("pq_public_key"),
        classical_priv_encrypted: row.get("classical_priv_encrypted"),
        pq_priv_encrypted: row.get("pq_priv_encrypted"),
        key_version: row.get::<_, i32>("key_version") as u32,
        created_at: row.get("created_at"),
        subscription_tier: parse_subscription_tier(&row.get::<_, String>("subscription_tier")),
        stripe_customer_id: row.get("stripe_customer_id"),
        stripe_subscription_id: row.get("stripe_subscription_id"),
        stripe_subscription_status: row.get("stripe_subscription_status"),
        subscription_current_period_end: row.get("subscription_current_period_end"),
        first_name: row.get("first_name"),
        last_name: row.get("last_name"),
        email: row.get("email"),
        is_locked: row.get("is_locked"),
        locked_reason: row.get("locked_reason"),
        admin_note: row.get("admin_note"),
        manual_subscription_tier: row.get::<_, Option<String>>("manual_subscription_tier").map(|value| SubscriptionTier::from_str(&value)),
        manual_subscription_expires_at: row.get("manual_subscription_expires_at"),
        manual_subscription_reason: row.get("manual_subscription_reason"),
        manual_subscription_granted_by: row.get("manual_subscription_granted_by"),
        access_grants: row.get::<_, Json<Vec<UserAccessGrant>>>("access_grants_json").0,
    })
}

pub(super) fn admin_audit_from_row(row: Row) -> Result<AdminAuditEvent, AppError> {
    Ok(AdminAuditEvent {
        id: None,
        public_id: row.get("id"),
        entity_type: row.get("entity_type"),
        entity_id: row.get("entity_id"),
        action_type: row.get("action_type"),
        summary: row.get("summary"),
        detail: row.get("detail"),
        actor: row.get("actor"),
        created_at: row.get("created_at"),
    })
}

pub(super) fn admin_user_from_row(row: Row) -> Result<AdminUserRecord, AppError> {
    Ok(AdminUserRecord {
        id: row.get("id"),
        username: row.get("username"),
        created_at: row.get("created_at"),
        subscription_tier: parse_subscription_tier(&row.get::<_, String>("subscription_tier")),
        stripe_customer_id: row.get("stripe_customer_id"),
        stripe_subscription_id: row.get("stripe_subscription_id"),
        stripe_subscription_status: row.get("stripe_subscription_status"),
        subscription_current_period_end: row.get("subscription_current_period_end"),
        first_name: row.get("first_name"),
        last_name: row.get("last_name"),
        email: row.get("email"),
        is_locked: row.get("is_locked"),
        locked_reason: row.get("locked_reason"),
        admin_note: row.get("admin_note"),
        manual_subscription_tier: row.get::<_, Option<String>>("manual_subscription_tier").map(|value| SubscriptionTier::from_str(&value)),
        manual_subscription_expires_at: row.get("manual_subscription_expires_at"),
        manual_subscription_reason: row.get("manual_subscription_reason"),
        manual_subscription_granted_by: row.get("manual_subscription_granted_by"),
        access_grants: row.get::<_, Json<Vec<UserAccessGrant>>>("access_grants_json").0,
    })
}

pub(super) fn stored_mind_map_from_row(row: Row) -> Result<StoredMindMap, AppError> {
    Ok(StoredMindMap {
        id: row.get("id"),
        user_id: row.get("user_id"),
        title_encrypted: row.get("title_encrypted"),
        object_key: row.get("object_key"),
        eph_classical_public: row.get("eph_classical_public"),
        eph_pq_ciphertext: row.get("eph_pq_ciphertext"),
        wrapped_dek: row.get("wrapped_dek"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        current_version_id: row.get("current_version_id"),
        version_history: row.get::<_, Json<Vec<VersionSnapshot>>>("version_history").0,
        vault_color: row.get("vault_color"),
        vault_note_encrypted: row.get("vault_note_encrypted"),
        vault_encryption_mode: row.get("vault_encryption_mode"),
        max_versions: row.get::<_, i32>("max_versions") as u32,
        vault_labels: row.get::<_, Json<Vec<String>>>("vault_labels").0,
    })
}

pub(super) fn parse_subscription_tier(value: &str) -> SubscriptionTier {
    SubscriptionTier::from_str(value)
}

pub(super) fn map_registration_invite(row: &Row) -> RegistrationInvite {
    RegistrationInvite {
        id: row.get("id"),
        code: row.get("code"),
        label: row.get("label"),
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
        used_at: row.get("used_at"),
        used_by_username: row.get("used_by_username"),
    }
}

pub(super) fn stored_mind_map_attachment_from_row(row: Row) -> Result<StoredMindMapAttachment, AppError> {
    Ok(StoredMindMapAttachment {
        id: row.get("id"),
        map_id: row.get("map_id"),
        node_id: row.get("node_id"),
        name: row.get("name"),
        sanitized_name: row.get("sanitized_name"),
        content_type: row.get("content_type"),
        size_bytes: row.get("size_bytes"),
        s3_key: row.get("s3_key"),
        s3_version_id: row.get("s3_version_id"),
        uploaded_by: row.get("uploaded_by"),
        uploaded_at: row.get("uploaded_at"),
        encrypted: row.get("encrypted"),
        encryption_meta: row.get::<_, Option<Json<serde_json::Value>>>("encryption_meta").map(|value| value.0),
        checksum_sha256: row.get("checksum_sha256"),
        status: AttachmentStatus::from_str(&row.get::<_, String>("status")),
    })
}

pub(super) fn user_account_settings_from_row(row: Row) -> Result<UserAccountSettings, AppError> {
    Ok(UserAccountSettings {
        locale: row.get("locale"),
        timezone: row.get("timezone"),
        date_format: row.get("date_format"),
        accessibility_reduce_motion: row.get("accessibility_reduce_motion"),
        sync_appearance_across_devices: row.get("sync_appearance_across_devices"),
        default_map_layout: row.get("default_map_layout"),
        default_map_theme: row.get("default_map_theme"),
        default_export_format: row.get("default_export_format"),
        default_node_style_preset: row.get("default_node_style_preset"),
        user_labels_json: row.try_get("user_labels_json").unwrap_or_else(|_| "[]".to_string()),
        updated_at: row.get("updated_at"),
    })
}
