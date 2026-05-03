use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use chrono::Utc;
use tokio_postgres::{types::Json, Client, NoTls, Row};

use crate::{
    config::AppConfig,
    db::sql_store::{
        AdminFeedbackRecord, AdminUserAdminUpdate, AdminUserRecord, ManualSubscriptionUpdate,
        MindMapAttachmentUploadUpdate, MindMapContentUpdate, MindMapMetaUpdate,
        MindMapShareAttachmentUploadUpdate, MindMapShareUploadUpdate, NewMindMap,
        NewMindMapAttachment, NewMindMapShare, NewMindMapShareAttachment, NewPlainTextMap,
        NewSharedUserGroup, NewUser, PlainTextMapUpdate, RotateCredentialsUpdate,
        SharedUserGroupUpdate, SqlStore,
        StoredMindMap, StoredMindMapAttachment, StoredMindMapShare,
        StoredMindMapShareAttachment, StoredPlainTextMap, StoredSharedUserGroup, StoredUser,
        UserProfileUpdate,
    },
    error::AppError,
    models::{
        access::UserAccessGrant,
        admin_audit::AdminAuditEvent,
        attachment::AttachmentStatus,
        feedback::NewFeedbackSubmission,
        mindmap::VersionSnapshot,
        notifications::{
            NewNotificationEvent, NotificationPriority, StoredNotificationEvent,
            UserNotificationSettings,
        },
        plaintext_map::{DirectUserShare, GroupMember, GroupShare},
        share::{ShareScope, ShareStatus},
        settings::UserAccountSettings,
        user::{Argon2Params, SubscriptionTier},
    },
};

#[derive(Clone)]
pub struct PostgresDb {
    client: Arc<Client>,
}

impl PostgresDb {
    pub async fn connect(cfg: &AppConfig) -> anyhow::Result<Self> {
        let dsn = if !cfg.sql_dsn.trim().is_empty() {
            cfg.sql_dsn.trim()
        } else {
            cfg.postgres_dsn.trim()
        };
        if dsn.is_empty() {
            anyhow::bail!("SQL_DSN is required when DB_ENGINE=sql (POSTGRES_DSN is accepted for backward compatibility)");
        }

        tracing::info!("Connecting to SQL backend");

        let (client, connection) = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_postgres::connect(dsn, NoTls),
        )
            .await
            .context("timed out while connecting to SQL backend")?
            .context("failed to connect to SQL backend")?;

        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!("SQL backend connection error: {error}");
            }
        });

        let this = Self {
            client: Arc::new(client),
        };
        tracing::info!("Ensuring SQL schema");
        tokio::time::timeout(Duration::from_secs(10), this.ensure_schema())
            .await
            .context("timed out while ensuring SQL schema")??;
        tracing::info!("Connected to SQL backend");
        Ok(this)
    }

    async fn ensure_schema(&self) -> anyhow::Result<()> {
        self.client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS users (
                    id TEXT PRIMARY KEY,
                    username TEXT NOT NULL UNIQUE,
                    auth_hash TEXT NOT NULL,
                    argon2_salt TEXT NOT NULL,
                    argon2_params JSONB NOT NULL,
                    classical_public_key TEXT NOT NULL,
                    pq_public_key TEXT NOT NULL,
                    classical_priv_encrypted TEXT NOT NULL,
                    pq_priv_encrypted TEXT NOT NULL,
                    key_version INTEGER NOT NULL,
                    created_at TIMESTAMPTZ NOT NULL,
                    subscription_tier TEXT NOT NULL,
                    stripe_customer_id TEXT,
                    stripe_subscription_id TEXT,
                    stripe_subscription_status TEXT,
                    subscription_current_period_end TIMESTAMPTZ,
                    first_name TEXT,
                    last_name TEXT,
                    email TEXT,
                    is_locked BOOLEAN NOT NULL DEFAULT FALSE,
                    locked_reason TEXT,
                    admin_note TEXT,
                    manual_subscription_tier TEXT,
                    manual_subscription_expires_at TIMESTAMPTZ,
                    manual_subscription_reason TEXT,
                    manual_subscription_granted_by TEXT,
                    access_grants_json JSONB NOT NULL DEFAULT '[]'::jsonb
                );

                ALTER TABLE users
                ADD COLUMN IF NOT EXISTS is_locked BOOLEAN NOT NULL DEFAULT FALSE;

                ALTER TABLE users ADD COLUMN IF NOT EXISTS locked_reason TEXT;
                ALTER TABLE users ADD COLUMN IF NOT EXISTS admin_note TEXT;
                ALTER TABLE users ADD COLUMN IF NOT EXISTS manual_subscription_tier TEXT;
                ALTER TABLE users ADD COLUMN IF NOT EXISTS manual_subscription_expires_at TIMESTAMPTZ;
                ALTER TABLE users ADD COLUMN IF NOT EXISTS manual_subscription_reason TEXT;
                ALTER TABLE users ADD COLUMN IF NOT EXISTS manual_subscription_granted_by TEXT;
                ALTER TABLE users ADD COLUMN IF NOT EXISTS access_grants_json JSONB NOT NULL DEFAULT '[]'::jsonb;

                CREATE TABLE IF NOT EXISTS user_account_settings (
                    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
                    locale TEXT NOT NULL DEFAULT 'en',
                    timezone TEXT NOT NULL DEFAULT 'UTC',
                    date_format TEXT NOT NULL DEFAULT 'iso',
                    accessibility_reduce_motion BOOLEAN NOT NULL DEFAULT FALSE,
                    sync_appearance_across_devices BOOLEAN NOT NULL DEFAULT FALSE,
                    default_share_expiry_days INTEGER NOT NULL DEFAULT 7,
                    default_include_attachments_on_share BOOLEAN NOT NULL DEFAULT FALSE,
                    default_map_layout TEXT NOT NULL DEFAULT 'mindmap',
                    default_map_theme TEXT NOT NULL DEFAULT 'system',
                    default_export_format TEXT NOT NULL DEFAULT 'cryptmind',
                    default_node_style_preset TEXT NOT NULL DEFAULT 'default',
                    user_labels_json TEXT NOT NULL DEFAULT '[]',
                    updated_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS user_notification_settings (
                    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
                    inbox_enabled BOOLEAN NOT NULL DEFAULT TRUE,
                    email_enabled BOOLEAN NOT NULL DEFAULT FALSE,
                    push_enabled BOOLEAN NOT NULL DEFAULT FALSE,
                    desktop_enabled BOOLEAN NOT NULL DEFAULT TRUE,
                    digest_enabled BOOLEAN NOT NULL DEFAULT FALSE,
                    quiet_hours_start TEXT,
                    quiet_hours_end TEXT,
                    allow_preview_local_only BOOLEAN NOT NULL DEFAULT TRUE,
                    share_created BOOLEAN NOT NULL DEFAULT TRUE,
                    share_revoked BOOLEAN NOT NULL DEFAULT TRUE,
                    attachment_upload_failures BOOLEAN NOT NULL DEFAULT TRUE,
                    billing_notices BOOLEAN NOT NULL DEFAULT TRUE,
                    security_alerts BOOLEAN NOT NULL DEFAULT TRUE,
                    admin_messages BOOLEAN NOT NULL DEFAULT TRUE,
                    collaboration_mentions BOOLEAN NOT NULL DEFAULT FALSE,
                    updated_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS notification_events (
                    id TEXT PRIMARY KEY,
                    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    event_type TEXT NOT NULL,
                    category TEXT NOT NULL,
                    priority TEXT NOT NULL,
                    actor_user_id TEXT,
                    object_type TEXT NOT NULL,
                    object_id TEXT NOT NULL,
                    object_label_safe TEXT,
                    reason_code TEXT NOT NULL,
                    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
                    created_at TIMESTAMPTZ NOT NULL,
                    read_at TIMESTAMPTZ,
                    saved_at TIMESTAMPTZ,
                    done_at TIMESTAMPTZ
                );

                CREATE TABLE IF NOT EXISTS mind_maps (
                    id TEXT PRIMARY KEY,
                    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    title_encrypted TEXT NOT NULL,
                    minio_object_key TEXT NOT NULL,
                    eph_classical_public TEXT NOT NULL,
                    eph_pq_ciphertext TEXT NOT NULL,
                    wrapped_dek TEXT NOT NULL,
                    created_at TIMESTAMPTZ NOT NULL,
                    updated_at TIMESTAMPTZ NOT NULL,
                    minio_version_id TEXT,
                    version_history JSONB NOT NULL DEFAULT '[]'::jsonb,
                    vault_color TEXT,
                    vault_note_encrypted TEXT,
                    vault_sharing_mode TEXT NOT NULL DEFAULT 'private',
                    vault_encryption_mode TEXT NOT NULL DEFAULT 'standard',
                    max_versions INTEGER NOT NULL,
                    vault_labels JSONB NOT NULL DEFAULT '[]'::jsonb
                );

                ALTER TABLE mind_maps ADD COLUMN IF NOT EXISTS vault_sharing_mode TEXT NOT NULL DEFAULT 'private';
                ALTER TABLE mind_maps ADD COLUMN IF NOT EXISTS vault_encryption_mode TEXT NOT NULL DEFAULT 'standard';
                ALTER TABLE mind_maps ADD COLUMN IF NOT EXISTS vault_labels JSONB NOT NULL DEFAULT '[]'::jsonb;
                ALTER TABLE user_account_settings ADD COLUMN IF NOT EXISTS user_labels_json TEXT NOT NULL DEFAULT '[]';

                CREATE TABLE IF NOT EXISTS mind_map_attachments (
                    id TEXT PRIMARY KEY,
                    map_id TEXT NOT NULL REFERENCES mind_maps(id) ON DELETE CASCADE,
                    node_id TEXT,
                    name TEXT NOT NULL,
                    sanitized_name TEXT NOT NULL,
                    content_type TEXT NOT NULL,
                    size_bytes BIGINT NOT NULL,
                    s3_key TEXT NOT NULL,
                    s3_version_id TEXT,
                    uploaded_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    uploaded_at TIMESTAMPTZ NOT NULL,
                    encrypted BOOLEAN NOT NULL,
                    encryption_meta JSONB,
                    checksum_sha256 TEXT,
                    status TEXT NOT NULL DEFAULT 'pending'
                );

                CREATE TABLE IF NOT EXISTS mind_map_shares (
                    id TEXT PRIMARY KEY,
                    map_id TEXT NOT NULL REFERENCES mind_maps(id) ON DELETE CASCADE,
                    share_name TEXT NOT NULL,
                    share_scope TEXT NOT NULL,
                    s3_key TEXT NOT NULL,
                    s3_version_id TEXT,
                    created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    created_at TIMESTAMPTZ NOT NULL,
                    updated_at TIMESTAMPTZ NOT NULL,
                    expires_at TIMESTAMPTZ,
                    revoked BOOLEAN NOT NULL DEFAULT FALSE,
                    include_attachments BOOLEAN NOT NULL DEFAULT FALSE,
                    passphrase_hint TEXT,
                    content_type TEXT NOT NULL,
                    size_bytes BIGINT NOT NULL,
                    encryption_meta JSONB NOT NULL,
                    checksum_sha256 TEXT,
                    status TEXT NOT NULL DEFAULT 'pending'
                );

                CREATE TABLE IF NOT EXISTS mind_map_share_attachments (
                    id TEXT PRIMARY KEY,
                    share_id TEXT NOT NULL REFERENCES mind_map_shares(id) ON DELETE CASCADE,
                    source_attachment_id TEXT,
                    node_id TEXT,
                    name TEXT NOT NULL,
                    sanitized_name TEXT NOT NULL,
                    content_type TEXT NOT NULL,
                    size_bytes BIGINT NOT NULL,
                    s3_key TEXT NOT NULL,
                    s3_version_id TEXT,
                    uploaded_at TIMESTAMPTZ NOT NULL,
                    encryption_meta JSONB NOT NULL,
                    checksum_sha256 TEXT,
                    status TEXT NOT NULL DEFAULT 'pending'
                );

                CREATE TABLE IF NOT EXISTS user_groups (
                    id TEXT PRIMARY KEY,
                    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    owner_username TEXT NOT NULL,
                    name TEXT NOT NULL,
                    description TEXT,
                    members_json JSONB NOT NULL DEFAULT '[]'::jsonb,
                    created_at TIMESTAMPTZ NOT NULL,
                    updated_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS plaintext_maps (
                    id TEXT PRIMARY KEY,
                    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                    owner_username TEXT NOT NULL,
                    title TEXT NOT NULL,
                    summary TEXT,
                    content_json JSONB NOT NULL,
                    direct_user_shares_json JSONB NOT NULL DEFAULT '[]'::jsonb,
                    group_shares_json JSONB NOT NULL DEFAULT '[]'::jsonb,
                    created_at TIMESTAMPTZ NOT NULL,
                    updated_at TIMESTAMPTZ NOT NULL
                );

                CREATE TABLE IF NOT EXISTS feedback_submissions (
                    id TEXT PRIMARY KEY,
                    name TEXT,
                    email TEXT,
                    subject TEXT NOT NULL,
                    message TEXT NOT NULL,
                    page_url TEXT,
                    created_at TIMESTAMPTZ NOT NULL,
                    is_archived BOOLEAN NOT NULL DEFAULT FALSE,
                    archived_at TIMESTAMPTZ
                );

                ALTER TABLE feedback_submissions ADD COLUMN IF NOT EXISTS is_archived BOOLEAN NOT NULL DEFAULT FALSE;
                ALTER TABLE feedback_submissions ADD COLUMN IF NOT EXISTS archived_at TIMESTAMPTZ;

                CREATE TABLE IF NOT EXISTS admin_audit_events (
                    id TEXT PRIMARY KEY,
                    entity_type TEXT NOT NULL,
                    entity_id TEXT NOT NULL,
                    action_type TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    detail TEXT,
                    actor TEXT,
                    created_at TIMESTAMPTZ NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_mind_maps_user_id ON mind_maps (user_id);
                CREATE INDEX IF NOT EXISTS idx_mind_map_attachments_map_id ON mind_map_attachments (map_id, uploaded_at DESC);
                CREATE INDEX IF NOT EXISTS idx_mind_map_attachments_uploaded_by ON mind_map_attachments (uploaded_by);
                CREATE INDEX IF NOT EXISTS idx_mind_map_attachments_status ON mind_map_attachments (status);
                CREATE INDEX IF NOT EXISTS idx_mind_map_shares_map_id ON mind_map_shares (map_id, created_at DESC);
                CREATE INDEX IF NOT EXISTS idx_mind_map_shares_status ON mind_map_shares (status, revoked);
                CREATE INDEX IF NOT EXISTS idx_mind_map_shares_expires_at ON mind_map_shares (expires_at);
                CREATE INDEX IF NOT EXISTS idx_mind_map_share_attachments_share_id ON mind_map_share_attachments (share_id, uploaded_at DESC);
                CREATE INDEX IF NOT EXISTS idx_mind_map_share_attachments_status ON mind_map_share_attachments (status);
                CREATE INDEX IF NOT EXISTS idx_user_groups_owner_user_id ON user_groups (owner_user_id);
                CREATE INDEX IF NOT EXISTS idx_plaintext_maps_owner_user_id ON plaintext_maps (owner_user_id);
                CREATE INDEX IF NOT EXISTS idx_notification_events_user_created_at ON notification_events (user_id, created_at DESC);
                CREATE INDEX IF NOT EXISTS idx_notification_events_user_unread ON notification_events (user_id, read_at, created_at DESC);
                CREATE INDEX IF NOT EXISTS idx_notification_events_user_category ON notification_events (user_id, category, created_at DESC);
                CREATE INDEX IF NOT EXISTS idx_feedback_submissions_created_at ON feedback_submissions (created_at DESC);
                CREATE INDEX IF NOT EXISTS idx_admin_audit_events_created_at ON admin_audit_events (created_at DESC);",
            )
            .await
            .context("failed to ensure PostgreSQL schema")?;

        Ok(())
    }
}

#[async_trait]
impl SqlStore for PostgresDb {
    async fn create_feedback_submission(
        &self,
        submission: NewFeedbackSubmission,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO feedback_submissions (
                    id, name, email, subject, message, page_url, created_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7
                 )",
                &[
                    &submission.public_id,
                    &submission.name,
                    &submission.email,
                    &submission.subject,
                    &submission.message,
                    &submission.page_url,
                    &submission.created_at,
                ],
            )
            .await?;

        Ok(())
    }

    async fn count_feedback_submissions(&self) -> Result<u64, AppError> {
        let row = self
            .client
            .query_one("SELECT COUNT(*)::BIGINT AS count FROM feedback_submissions", &[])
            .await?;

        Ok(row.get::<_, i64>("count") as u64)
    }

    async fn list_admin_feedback(&self, limit: usize) -> Result<Vec<AdminFeedbackRecord>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, name, email, subject, message, page_url, created_at, is_archived, archived_at
                 FROM feedback_submissions
                 ORDER BY created_at DESC
                 LIMIT $1",
                &[&(limit as i64)],
            )
            .await?;

        rows.into_iter().map(admin_feedback_from_row).collect()
    }

    async fn delete_feedback_submission(&self, public_id: &str) -> Result<bool, AppError> {
        let deleted = self
            .client
            .execute(
                "DELETE FROM feedback_submissions WHERE id = $1",
                &[&public_id],
            )
            .await?;

        Ok(deleted > 0)
    }

    async fn set_feedback_archived(&self, public_id: &str, archived: bool) -> Result<bool, AppError> {
        let archived_at = if archived { Some(Utc::now()) } else { None };
        let updated = self
            .client
            .execute(
                "UPDATE feedback_submissions
                 SET is_archived = $1,
                     archived_at = $2
                 WHERE id = $3",
                &[&archived, &archived_at, &public_id],
            )
            .await?;

        Ok(updated > 0)
    }

    async fn list_admin_audit_events(&self, limit: usize) -> Result<Vec<AdminAuditEvent>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, entity_type, entity_id, action_type, summary, detail, actor, created_at
                 FROM admin_audit_events
                 ORDER BY created_at DESC
                 LIMIT $1",
                &[&(limit as i64)],
            )
            .await?;

        rows.into_iter().map(admin_audit_from_row).collect()
    }

    async fn create_admin_audit_event(&self, event: AdminAuditEvent) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO admin_audit_events (
                    id, entity_type, entity_id, action_type, summary, detail, actor, created_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8
                 )",
                &[
                    &event.public_id,
                    &event.entity_type,
                    &event.entity_id,
                    &event.action_type,
                    &event.summary,
                    &event.detail,
                    &event.actor,
                    &event.created_at,
                ],
            )
            .await?;

        Ok(())
    }

    async fn list_admin_users(&self) -> Result<Vec<AdminUserRecord>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT
                    id, username, created_at, subscription_tier, stripe_customer_id,
                    stripe_subscription_id, stripe_subscription_status, subscription_current_period_end,
                    first_name, last_name, email, is_locked, locked_reason, admin_note,
                    manual_subscription_tier, manual_subscription_expires_at, manual_subscription_reason,
                          manual_subscription_granted_by, access_grants_json
                 FROM users
                 ORDER BY created_at DESC",
                &[],
            )
            .await?;

        rows.into_iter().map(admin_user_from_row).collect()
    }

    async fn load_user_by_username(&self, username: &str) -> Result<Option<StoredUser>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT
                    id, username, auth_hash, argon2_salt, argon2_params,
                    classical_public_key, pq_public_key, classical_priv_encrypted, pq_priv_encrypted,
                    key_version, created_at, subscription_tier, stripe_customer_id,
                    stripe_subscription_id, stripe_subscription_status, subscription_current_period_end,
                    first_name, last_name, email, is_locked, locked_reason, admin_note,
                    manual_subscription_tier, manual_subscription_expires_at, manual_subscription_reason,
                          manual_subscription_granted_by, access_grants_json
                 FROM users
                 WHERE username = $1
                 LIMIT 1",
                &[&username],
            )
            .await?;

        row.map(stored_user_from_row).transpose()
    }

    async fn load_user_by_id(&self, id: &str) -> Result<Option<StoredUser>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT
                    id, username, auth_hash, argon2_salt, argon2_params,
                    classical_public_key, pq_public_key, classical_priv_encrypted, pq_priv_encrypted,
                    key_version, created_at, subscription_tier, stripe_customer_id,
                    stripe_subscription_id, stripe_subscription_status, subscription_current_period_end,
                    first_name, last_name, email, is_locked, locked_reason, admin_note,
                    manual_subscription_tier, manual_subscription_expires_at, manual_subscription_reason,
                          manual_subscription_granted_by, access_grants_json
                 FROM users
                 WHERE id = $1
                 LIMIT 1",
                &[&id],
            )
            .await?;

        row.map(stored_user_from_row).transpose()
    }

    async fn create_user(&self, user: NewUser) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO users (
                    id, username, auth_hash, argon2_salt, argon2_params,
                    classical_public_key, pq_public_key, classical_priv_encrypted, pq_priv_encrypted,
                    key_version, created_at, subscription_tier, stripe_customer_id,
                    stripe_subscription_id, stripe_subscription_status, subscription_current_period_end,
                    first_name, last_name, email, is_locked, locked_reason, admin_note,
                    manual_subscription_tier, manual_subscription_expires_at, manual_subscription_reason,
                    manual_subscription_granted_by, access_grants_json
                ) VALUES (
                    $1, $2, $3, $4, $5,
                    $6, $7, $8, $9,
                    $10, $11, $12, $13,
                    $14, $15, $16,
                    $17, $18, $19, $20,
                    $21, $22, $23, $24,
                    $25, $26, $27
                )",
                &[
                    &user.id,
                    &user.username,
                    &user.auth_hash,
                    &user.argon2_salt,
                    &Json(&user.argon2_params),
                    &user.classical_public_key,
                    &user.pq_public_key,
                    &user.classical_priv_encrypted,
                    &user.pq_priv_encrypted,
                    &(user.key_version as i32),
                    &user.created_at,
                    &user.subscription_tier.as_str(),
                    &user.stripe_customer_id,
                    &user.stripe_subscription_id,
                    &user.stripe_subscription_status,
                    &user.subscription_current_period_end,
                    &user.first_name,
                    &user.last_name,
                    &user.email,
                    &user.is_locked,
                    &user.locked_reason,
                    &user.admin_note,
                    &user.manual_subscription_tier.map(|value| value.as_str().to_string()),
                    &user.manual_subscription_expires_at,
                    &user.manual_subscription_reason,
                    &user.manual_subscription_granted_by,
                    &Json(&user.access_grants),
                ],
            )
            .await?;

        Ok(())
    }

    async fn update_user_profile(
        &self,
        user_id: &str,
        update: UserProfileUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE users
                 SET first_name = $1, last_name = $2, email = $3
                 WHERE id = $4",
                &[&update.first_name, &update.last_name, &update.email, &user_id],
            )
            .await?;

        Ok(())
    }

    async fn rotate_user_credentials(
        &self,
        user_id: &str,
        update: RotateCredentialsUpdate,
    ) -> Result<(), AppError> {
        use argon2::{
            password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
            Argon2,
        };

        // Hash new_auth_token before touching the DB — raw token must never rest on disk.
        let salt = SaltString::generate(&mut OsRng);
        let new_auth_hash = Argon2::default()
            .hash_password(update.new_auth_token.as_bytes(), &salt)
            .map_err(|e| AppError::Internal(format!("argon2 hash error: {e}")))?
            .to_string();

        // Explicit transaction. We cannot call client.transaction() because the
        // client is behind Arc<Client>. On a single connection, all statements
        // between BEGIN and COMMIT are serialised, giving us all-or-nothing
        // semantics: credential row + every vault title either all commit
        // together or nothing changes.
        self.client
            .execute("BEGIN", &[])
            .await
            .map_err(|e| AppError::Internal(format!("begin transaction failed: {e}")))?;

        let result: Result<(), AppError> = async {
            let affected = self
                .client
                .execute(
                    "UPDATE users
                     SET auth_hash                = $1,
                         argon2_salt              = $2,
                         argon2_params            = $3,
                         classical_priv_encrypted = $4,
                         pq_priv_encrypted        = $5,
                         key_version              = $6
                     WHERE id = $7",
                    &[
                        &new_auth_hash,
                        &update.new_argon2_salt,
                        &Json(&update.new_argon2_params),
                        &update.new_classical_priv_encrypted,
                        &update.new_pq_priv_encrypted,
                        &(update.new_key_version as i32),
                        &user_id,
                    ],
                )
                .await?;

            if affected == 0 {
                return Err(AppError::NotFound("user not found".to_string()));
            }

            // The route handler enforces complete coverage — every vault owned by
            // this user must appear in updated_vaults — so after the transaction
            // commits no vault retains a title encrypted with the old key.
            for vault in &update.updated_vaults {
                let note_value: Option<String> = vault
                    .vault_note_encrypted
                    .as_ref()
                    .and_then(|n| if n.is_empty() { None } else { Some(n.clone()) });

                self.client
                    .execute(
                        "UPDATE mind_maps
                         SET title_encrypted      = $1,
                             vault_note_encrypted = CASE WHEN $2::boolean
                                                         THEN $3::text
                                                         ELSE vault_note_encrypted
                                                    END
                         WHERE id = $4 AND user_id = $5",
                        &[
                            &vault.title_encrypted,
                            &vault.vault_note_encrypted.is_some(),
                            &note_value,
                            &vault.id,
                            &user_id,
                        ],
                    )
                    .await?;
            }

            Ok(())
        }
        .await;

        match result {
            Ok(()) => {
                self.client
                    .execute("COMMIT", &[])
                    .await
                    .map_err(|e| AppError::Internal(format!("commit failed: {e}")))?;
                Ok(())
            }
            Err(e) => {
                let _ = self.client.execute("ROLLBACK", &[]).await;
                Err(e)
            }
        }
    }

    async fn update_user_stripe_customer_id(
        &self,
        user_id: &str,
        stripe_customer_id: &str,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE users
                 SET stripe_customer_id = $1
                 WHERE id = $2",
                &[&stripe_customer_id, &user_id],
            )
            .await?;

        Ok(())
    }

    async fn update_user_subscription_by_customer_id(
        &self,
        stripe_customer_id: &str,
        subscription_tier: SubscriptionTier,
        stripe_subscription_status: Option<String>,
        subscription_current_period_end: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE users
                 SET subscription_tier = $1,
                     stripe_subscription_status = $2,
                     subscription_current_period_end = $3
                 WHERE stripe_customer_id = $4",
                &[
                    &subscription_tier.as_str(),
                    &stripe_subscription_status,
                    &subscription_current_period_end,
                    &stripe_customer_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn delete_user(&self, user_id: &str) -> Result<(), AppError> {
        self.client
            .execute(
                "DELETE FROM users WHERE id = $1",
                &[&user_id],
            )
            .await?;

        Ok(())
    }

    async fn set_user_locked(&self, user_id: &str, is_locked: bool) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE users
                 SET is_locked = $1
                 WHERE id = $2",
                &[&is_locked, &user_id],
            )
            .await?;

        Ok(())
    }

    async fn update_user_admin_fields(
        &self,
        user_id: &str,
        update: AdminUserAdminUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE users
                 SET admin_note = $1,
                     locked_reason = $2
                 WHERE id = $3",
                &[&update.admin_note, &update.locked_reason, &user_id],
            )
            .await?;

        Ok(())
    }

    async fn update_user_manual_subscription(
        &self,
        user_id: &str,
        update: ManualSubscriptionUpdate,
    ) -> Result<(), AppError> {
        let manual_subscription_tier = update.manual_subscription_tier.map(|value| value.as_str().to_string());
        self.client
            .execute(
                "UPDATE users
                 SET manual_subscription_tier = $1,
                     manual_subscription_expires_at = $2,
                     manual_subscription_reason = $3,
                     manual_subscription_granted_by = $4
                 WHERE id = $5",
                &[
                    &manual_subscription_tier,
                    &update.manual_subscription_expires_at,
                    &update.manual_subscription_reason,
                    &update.manual_subscription_granted_by,
                    &user_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn update_user_access_grants(
        &self,
        user_id: &str,
        access_grants: Vec<UserAccessGrant>,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE users
                 SET access_grants_json = $1
                 WHERE id = $2",
                &[&Json(&access_grants), &user_id],
            )
            .await?;

        Ok(())
    }

    async fn load_user_account_settings(
        &self,
        user_id: &str,
    ) -> Result<Option<UserAccountSettings>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT user_id, locale, timezone, date_format, accessibility_reduce_motion,
                        sync_appearance_across_devices, default_share_expiry_days,
                        default_include_attachments_on_share, default_map_layout,
                        default_map_theme, default_export_format, default_node_style_preset,
                        user_labels_json, updated_at
                 FROM user_account_settings
                 WHERE user_id = $1
                 LIMIT 1",
                &[&user_id],
            )
            .await?;

        row.map(user_account_settings_from_row).transpose()
    }

    async fn upsert_user_account_settings(
        &self,
        user_id: &str,
        settings: UserAccountSettings,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO user_account_settings (
                    user_id, locale, timezone, date_format, accessibility_reduce_motion,
                    sync_appearance_across_devices, default_share_expiry_days,
                    default_include_attachments_on_share, default_map_layout,
                    default_map_theme, default_export_format, default_node_style_preset,
                    user_labels_json, updated_at
                 ) VALUES (
                    $1, $2, $3, $4, $5,
                    $6, $7, $8, $9,
                    $10, $11, $12,
                    $13, $14
                 )
                 ON CONFLICT (user_id) DO UPDATE SET
                    locale = EXCLUDED.locale,
                    timezone = EXCLUDED.timezone,
                    date_format = EXCLUDED.date_format,
                    accessibility_reduce_motion = EXCLUDED.accessibility_reduce_motion,
                    sync_appearance_across_devices = EXCLUDED.sync_appearance_across_devices,
                    default_share_expiry_days = EXCLUDED.default_share_expiry_days,
                    default_include_attachments_on_share = EXCLUDED.default_include_attachments_on_share,
                    default_map_layout = EXCLUDED.default_map_layout,
                    default_map_theme = EXCLUDED.default_map_theme,
                    default_export_format = EXCLUDED.default_export_format,
                    default_node_style_preset = EXCLUDED.default_node_style_preset,
                    user_labels_json = EXCLUDED.user_labels_json,
                    updated_at = EXCLUDED.updated_at",
                &[
                    &user_id,
                    &settings.locale,
                    &settings.timezone,
                    &settings.date_format,
                    &settings.accessibility_reduce_motion,
                    &settings.sync_appearance_across_devices,
                    &settings.default_share_expiry_days,
                    &settings.default_include_attachments_on_share,
                    &settings.default_map_layout,
                    &settings.default_map_theme,
                    &settings.default_export_format,
                    &settings.default_node_style_preset,
                    &settings.user_labels_json,
                    &settings.updated_at,
                ],
            )
            .await?;

        Ok(())
    }

    async fn load_user_notification_settings(
        &self,
        user_id: &str,
    ) -> Result<Option<UserNotificationSettings>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT user_id, inbox_enabled, email_enabled, push_enabled, desktop_enabled,
                        digest_enabled, quiet_hours_start, quiet_hours_end,
                        allow_preview_local_only, share_created, share_revoked,
                        attachment_upload_failures, billing_notices, security_alerts,
                        admin_messages, collaboration_mentions, updated_at
                 FROM user_notification_settings
                 WHERE user_id = $1
                 LIMIT 1",
                &[&user_id],
            )
            .await?;

        row.map(user_notification_settings_from_row).transpose()
    }

    async fn upsert_user_notification_settings(
        &self,
        user_id: &str,
        settings: UserNotificationSettings,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO user_notification_settings (
                    user_id, inbox_enabled, email_enabled, push_enabled, desktop_enabled,
                    digest_enabled, quiet_hours_start, quiet_hours_end,
                    allow_preview_local_only, share_created, share_revoked,
                    attachment_upload_failures, billing_notices, security_alerts,
                    admin_messages, collaboration_mentions, updated_at
                 ) VALUES (
                    $1, $2, $3, $4, $5,
                    $6, $7, $8,
                    $9, $10, $11,
                    $12, $13, $14,
                    $15, $16, $17
                 )
                 ON CONFLICT (user_id) DO UPDATE SET
                    inbox_enabled = EXCLUDED.inbox_enabled,
                    email_enabled = EXCLUDED.email_enabled,
                    push_enabled = EXCLUDED.push_enabled,
                    desktop_enabled = EXCLUDED.desktop_enabled,
                    digest_enabled = EXCLUDED.digest_enabled,
                    quiet_hours_start = EXCLUDED.quiet_hours_start,
                    quiet_hours_end = EXCLUDED.quiet_hours_end,
                    allow_preview_local_only = EXCLUDED.allow_preview_local_only,
                    share_created = EXCLUDED.share_created,
                    share_revoked = EXCLUDED.share_revoked,
                    attachment_upload_failures = EXCLUDED.attachment_upload_failures,
                    billing_notices = EXCLUDED.billing_notices,
                    security_alerts = EXCLUDED.security_alerts,
                    admin_messages = EXCLUDED.admin_messages,
                    collaboration_mentions = EXCLUDED.collaboration_mentions,
                    updated_at = EXCLUDED.updated_at",
                &[
                    &user_id,
                    &settings.inbox_enabled,
                    &settings.email_enabled,
                    &settings.push_enabled,
                    &settings.desktop_enabled,
                    &settings.digest_enabled,
                    &settings.quiet_hours_start,
                    &settings.quiet_hours_end,
                    &settings.allow_preview_local_only,
                    &settings.share_created,
                    &settings.share_revoked,
                    &settings.attachment_upload_failures,
                    &settings.billing_notices,
                    &settings.security_alerts,
                    &settings.admin_messages,
                    &settings.collaboration_mentions,
                    &settings.updated_at,
                ],
            )
            .await?;

        Ok(())
    }

    async fn list_notification_events(
        &self,
        user_id: &str,
        category: Option<&str>,
        state: Option<&str>,
        limit: usize,
    ) -> Result<Vec<StoredNotificationEvent>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, user_id, event_type, category, priority, actor_user_id,
                        object_type, object_id, object_label_safe, reason_code,
                        payload_json, created_at, read_at, saved_at, done_at
                 FROM notification_events
                 WHERE user_id = $1
                   AND ($2::TEXT IS NULL OR category = $2)
                   AND (
                        $3::TEXT IS NULL
                        OR $3 = 'all'
                        OR ($3 = 'unread' AND read_at IS NULL)
                        OR ($3 = 'saved' AND saved_at IS NOT NULL)
                        OR ($3 = 'done' AND done_at IS NOT NULL)
                   )
                 ORDER BY created_at DESC
                 LIMIT $4",
                &[&user_id, &category, &state, &(limit as i64)],
            )
            .await?;

        rows.into_iter().map(notification_event_from_row).collect()
    }

    async fn create_notification_event(
        &self,
        event: NewNotificationEvent,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO notification_events (
                    id, user_id, event_type, category, priority, actor_user_id,
                    object_type, object_id, object_label_safe, reason_code,
                    payload_json, created_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6,
                    $7, $8, $9, $10,
                    $11, $12
                 )",
                &[
                    &event.id,
                    &event.user_id,
                    &event.event_type,
                    &event.category,
                    &event.priority.as_str(),
                    &event.actor_user_id,
                    &event.object_type,
                    &event.object_id,
                    &event.object_label_safe,
                    &event.reason_code,
                    &Json(&event.payload_json),
                    &event.created_at,
                ],
            )
            .await?;

        Ok(())
    }

    async fn mark_notification_read(
        &self,
        user_id: &str,
        notification_id: &str,
        read: bool,
    ) -> Result<bool, AppError> {
        let read_at = if read { Some(Utc::now()) } else { None };
        let updated = self
            .client
            .execute(
                "UPDATE notification_events
                 SET read_at = $1
                 WHERE user_id = $2 AND id = $3",
                &[&read_at, &user_id, &notification_id],
            )
            .await?;

        Ok(updated > 0)
    }

    async fn mark_notification_saved(
        &self,
        user_id: &str,
        notification_id: &str,
        saved: bool,
    ) -> Result<bool, AppError> {
        let saved_at = if saved { Some(Utc::now()) } else { None };
        let updated = self
            .client
            .execute(
                "UPDATE notification_events
                 SET saved_at = $1
                 WHERE user_id = $2 AND id = $3",
                &[&saved_at, &user_id, &notification_id],
            )
            .await?;

        Ok(updated > 0)
    }

    async fn mark_notification_done(
        &self,
        user_id: &str,
        notification_id: &str,
        done: bool,
    ) -> Result<bool, AppError> {
        let done_at = if done { Some(Utc::now()) } else { None };
        let updated = self
            .client
            .execute(
                "UPDATE notification_events
                 SET done_at = $1
                 WHERE user_id = $2 AND id = $3",
                &[&done_at, &user_id, &notification_id],
            )
            .await?;

        Ok(updated > 0)
    }

    async fn mark_all_notifications_read(&self, user_id: &str) -> Result<u64, AppError> {
        let updated = self
            .client
            .execute(
                "UPDATE notification_events
                 SET read_at = NOW()
                 WHERE user_id = $1 AND read_at IS NULL",
                &[&user_id],
            )
            .await?;

        Ok(updated)
    }

    async fn list_mind_maps(&self, user_id: &str) -> Result<Vec<StoredMindMap>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT
                    id, user_id, title_encrypted, minio_object_key, eph_classical_public,
                    eph_pq_ciphertext, wrapped_dek, created_at, updated_at, minio_version_id,
                    version_history, vault_color, vault_note_encrypted, vault_sharing_mode,
                    vault_encryption_mode, max_versions, vault_labels
                 FROM mind_maps
                 WHERE user_id = $1
                 ORDER BY updated_at DESC",
                &[&user_id],
            )
            .await?;

        rows.into_iter().map(stored_mind_map_from_row).collect()
    }

    async fn create_mind_map(&self, map: NewMindMap) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO mind_maps (
                    id, user_id, title_encrypted, minio_object_key, eph_classical_public,
                    eph_pq_ciphertext, wrapped_dek, created_at, updated_at, minio_version_id,
                    version_history, vault_color, vault_note_encrypted, vault_sharing_mode,
                    vault_encryption_mode, max_versions, vault_labels
                 ) VALUES (
                    $1, $2, $3, $4, $5,
                    $6, $7, $8, $9, $10,
                    $11, $12, $13, $14,
                    $15, $16, $17
                 )",
                &[
                    &map.id,
                    &map.user_id,
                    &map.title_encrypted,
                    &map.minio_object_key,
                    &map.eph_classical_public,
                    &map.eph_pq_ciphertext,
                    &map.wrapped_dek,
                    &map.created_at,
                    &map.updated_at,
                    &map.minio_version_id,
                    &Json(&map.version_history),
                    &map.vault_color,
                    &map.vault_note_encrypted,
                    &map.vault_sharing_mode,
                    &map.vault_encryption_mode,
                    &(map.max_versions as i32),
                    &Json(&map.vault_labels),
                ],
            )
            .await?;

        Ok(())
    }

    async fn get_mind_map_owned(
        &self,
        id: &str,
        user_id: &str,
    ) -> Result<Option<StoredMindMap>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT
                    id, user_id, title_encrypted, minio_object_key, eph_classical_public,
                    eph_pq_ciphertext, wrapped_dek, created_at, updated_at, minio_version_id,
                    version_history, vault_color, vault_note_encrypted, vault_sharing_mode,
                    vault_encryption_mode, max_versions, vault_labels
                 FROM mind_maps
                 WHERE id = $1 AND user_id = $2
                 LIMIT 1",
                &[&id, &user_id],
            )
            .await?;

        row.map(stored_mind_map_from_row).transpose()
    }

    async fn update_mind_map_content(
        &self,
        id: &str,
        user_id: &str,
        update: MindMapContentUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_maps
                 SET title_encrypted = $1,
                     eph_classical_public = $2,
                     eph_pq_ciphertext = $3,
                     wrapped_dek = $4,
                     updated_at = $5
                 WHERE id = $6 AND user_id = $7",
                &[
                    &update.title_encrypted,
                    &update.eph_classical_public,
                    &update.eph_pq_ciphertext,
                    &update.wrapped_dek,
                    &update.updated_at,
                    &id,
                    &user_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn update_mind_map_upload(
        &self,
        id: &str,
        user_id: &str,
        minio_version_id: &str,
        version_history: Vec<VersionSnapshot>,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_maps
                 SET minio_version_id = $1, version_history = $2
                 WHERE id = $3 AND user_id = $4",
                &[&minio_version_id, &Json(&version_history), &id, &user_id],
            )
            .await?;

        Ok(())
    }

    async fn delete_mind_map(&self, id: &str, user_id: &str) -> Result<(), AppError> {
        self.client
            .execute(
                "DELETE FROM mind_maps WHERE id = $1 AND user_id = $2",
                &[&id, &user_id],
            )
            .await?;

        Ok(())
    }

    async fn update_mind_map_meta(
        &self,
        id: &str,
        user_id: &str,
        update: MindMapMetaUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_maps
                 SET vault_color = $1,
                     vault_note_encrypted = $2,
                     vault_sharing_mode = $3,
                     vault_encryption_mode = $4,
                     max_versions = $5,
                     title_encrypted = $6,
                     updated_at = $7,
                     vault_labels = $8
                 WHERE id = $9 AND user_id = $10",
                &[
                    &update.vault_color,
                    &update.vault_note_encrypted,
                    &update.vault_sharing_mode,
                    &update.vault_encryption_mode,
                    &(update.max_versions as i32),
                    &update.title_encrypted,
                    &update.updated_at,
                    &Json(&update.vault_labels),
                    &id,
                    &user_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn list_mind_map_attachments(
        &self,
        map_id: &str,
    ) -> Result<Vec<StoredMindMapAttachment>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, map_id, node_id, name, sanitized_name, content_type, size_bytes,
                        s3_key, s3_version_id, uploaded_by, uploaded_at, encrypted,
                        encryption_meta, checksum_sha256, status
                 FROM mind_map_attachments
                 WHERE map_id = $1 AND status <> 'deleted'
                 ORDER BY uploaded_at DESC",
                &[&map_id],
            )
            .await?;

        rows.into_iter().map(stored_mind_map_attachment_from_row).collect()
    }

    async fn create_mind_map_attachment(
        &self,
        attachment: NewMindMapAttachment,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO mind_map_attachments (
                    id, map_id, node_id, name, sanitized_name, content_type, size_bytes,
                    s3_key, s3_version_id, uploaded_by, uploaded_at, encrypted,
                    encryption_meta, checksum_sha256, status
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7,
                    $8, $9, $10, $11, $12,
                    $13, $14, $15
                 )",
                &[
                    &attachment.id,
                    &attachment.map_id,
                    &attachment.node_id,
                    &attachment.name,
                    &attachment.sanitized_name,
                    &attachment.content_type,
                    &attachment.size_bytes,
                    &attachment.s3_key,
                    &attachment.s3_version_id,
                    &attachment.uploaded_by,
                    &attachment.uploaded_at,
                    &attachment.encrypted,
                    &attachment.encryption_meta.map(Json),
                    &attachment.checksum_sha256,
                    &attachment.status.as_str(),
                ],
            )
            .await?;

        Ok(())
    }

    async fn get_mind_map_attachment(
        &self,
        map_id: &str,
        attachment_id: &str,
    ) -> Result<Option<StoredMindMapAttachment>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT id, map_id, node_id, name, sanitized_name, content_type, size_bytes,
                        s3_key, s3_version_id, uploaded_by, uploaded_at, encrypted,
                        encryption_meta, checksum_sha256, status
                 FROM mind_map_attachments
                 WHERE map_id = $1 AND id = $2 AND status <> 'deleted'
                 LIMIT 1",
                &[&map_id, &attachment_id],
            )
            .await?;

        row.map(stored_mind_map_attachment_from_row).transpose()
    }

    async fn complete_mind_map_attachment_upload(
        &self,
        map_id: &str,
        attachment_id: &str,
        update: MindMapAttachmentUploadUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_attachments
                 SET s3_version_id = $1,
                     checksum_sha256 = $2,
                     status = $3
                 WHERE map_id = $4 AND id = $5 AND status <> 'deleted'",
                &[
                    &update.s3_version_id,
                    &update.checksum_sha256,
                    &update.status.as_str(),
                    &map_id,
                    &attachment_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn update_mind_map_attachment_node(
        &self,
        map_id: &str,
        attachment_id: &str,
        node_id: Option<String>,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_attachments
                 SET node_id = $1
                 WHERE map_id = $2 AND id = $3 AND status <> 'deleted'",
                &[&node_id, &map_id, &attachment_id],
            )
            .await?;

        Ok(())
    }

    async fn mark_mind_map_attachment_deleted(
        &self,
        map_id: &str,
        attachment_id: &str,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_attachments
                 SET status = 'deleted'
                 WHERE map_id = $1 AND id = $2 AND status <> 'deleted'",
                &[&map_id, &attachment_id],
            )
            .await?;

        Ok(())
    }

    async fn list_mind_map_shares(&self, map_id: &str) -> Result<Vec<StoredMindMapShare>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, map_id, share_name, share_scope, s3_key, s3_version_id, created_by,
                        created_at, updated_at, expires_at, revoked, include_attachments,
                        passphrase_hint, content_type, size_bytes, encryption_meta,
                        checksum_sha256, status
                 FROM mind_map_shares
                 WHERE map_id = $1
                 ORDER BY created_at DESC",
                &[&map_id],
            )
            .await?;

        rows.into_iter().map(stored_mind_map_share_from_row).collect()
    }

    async fn create_mind_map_share(&self, share: NewMindMapShare) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO mind_map_shares (
                    id, map_id, share_name, share_scope, s3_key, s3_version_id, created_by,
                    created_at, updated_at, expires_at, revoked, include_attachments,
                    passphrase_hint, content_type, size_bytes, encryption_meta,
                    checksum_sha256, status
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7,
                    $8, $9, $10, $11, $12,
                    $13, $14, $15, $16,
                    $17, $18
                 )",
                &[
                    &share.id,
                    &share.map_id,
                    &share.share_name,
                    &share.scope.as_str(),
                    &share.s3_key,
                    &share.s3_version_id,
                    &share.created_by,
                    &share.created_at,
                    &share.updated_at,
                    &share.expires_at,
                    &share.revoked,
                    &share.include_attachments,
                    &share.passphrase_hint,
                    &share.content_type,
                    &share.size_bytes,
                    &Json(&share.encryption_meta),
                    &share.checksum_sha256,
                    &share.status.as_str(),
                ],
            )
            .await?;

        Ok(())
    }

    async fn get_mind_map_share(
        &self,
        map_id: &str,
        share_id: &str,
    ) -> Result<Option<StoredMindMapShare>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT id, map_id, share_name, share_scope, s3_key, s3_version_id, created_by,
                        created_at, updated_at, expires_at, revoked, include_attachments,
                        passphrase_hint, content_type, size_bytes, encryption_meta,
                        checksum_sha256, status
                 FROM mind_map_shares
                 WHERE map_id = $1 AND id = $2
                 LIMIT 1",
                &[&map_id, &share_id],
            )
            .await?;

        row.map(stored_mind_map_share_from_row).transpose()
    }

    async fn get_public_mind_map_share(&self, share_id: &str) -> Result<Option<StoredMindMapShare>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT id, map_id, share_name, share_scope, s3_key, s3_version_id, created_by,
                        created_at, updated_at, expires_at, revoked, include_attachments,
                        passphrase_hint, content_type, size_bytes, encryption_meta,
                        checksum_sha256, status
                 FROM mind_map_shares
                 WHERE id = $1
                 LIMIT 1",
                &[&share_id],
            )
            .await?;

        row.map(stored_mind_map_share_from_row).transpose()
    }

    async fn complete_mind_map_share_upload(
        &self,
        map_id: &str,
        share_id: &str,
        update: MindMapShareUploadUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_shares
                 SET s3_version_id = $1,
                     checksum_sha256 = $2,
                     status = $3,
                     updated_at = NOW()
                 WHERE map_id = $4 AND id = $5",
                &[
                    &update.s3_version_id,
                    &update.checksum_sha256,
                    &update.status.as_str(),
                    &map_id,
                    &share_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn set_mind_map_share_revoked(
        &self,
        map_id: &str,
        share_id: &str,
        revoked: bool,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_shares
                 SET revoked = $1,
                     status = CASE WHEN $1 THEN 'revoked' ELSE status END,
                     updated_at = NOW()
                 WHERE map_id = $2 AND id = $3",
                &[&revoked, &map_id, &share_id],
            )
            .await?;

        Ok(())
    }

    async fn list_mind_map_share_attachments(
        &self,
        share_id: &str,
    ) -> Result<Vec<StoredMindMapShareAttachment>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, share_id, source_attachment_id, node_id, name, sanitized_name,
                        content_type, size_bytes, s3_key, s3_version_id, uploaded_at,
                        encryption_meta, checksum_sha256, status
                 FROM mind_map_share_attachments
                 WHERE share_id = $1 AND status <> 'deleted'
                 ORDER BY uploaded_at DESC",
                &[&share_id],
            )
            .await?;

        rows.into_iter()
            .map(stored_mind_map_share_attachment_from_row)
            .collect()
    }

    async fn create_mind_map_share_attachment(
        &self,
        attachment: NewMindMapShareAttachment,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO mind_map_share_attachments (
                    id, share_id, source_attachment_id, node_id, name, sanitized_name,
                    content_type, size_bytes, s3_key, s3_version_id, uploaded_at,
                    encryption_meta, checksum_sha256, status
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6,
                    $7, $8, $9, $10, $11,
                    $12, $13, $14
                 )",
                &[
                    &attachment.id,
                    &attachment.share_id,
                    &attachment.source_attachment_id,
                    &attachment.node_id,
                    &attachment.name,
                    &attachment.sanitized_name,
                    &attachment.content_type,
                    &attachment.size_bytes,
                    &attachment.s3_key,
                    &attachment.s3_version_id,
                    &attachment.uploaded_at,
                    &Json(&attachment.encryption_meta),
                    &attachment.checksum_sha256,
                    &attachment.status.as_str(),
                ],
            )
            .await?;

        Ok(())
    }

    async fn get_mind_map_share_attachment(
        &self,
        share_id: &str,
        attachment_id: &str,
    ) -> Result<Option<StoredMindMapShareAttachment>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT id, share_id, source_attachment_id, node_id, name, sanitized_name,
                        content_type, size_bytes, s3_key, s3_version_id, uploaded_at,
                        encryption_meta, checksum_sha256, status
                 FROM mind_map_share_attachments
                 WHERE share_id = $1 AND id = $2 AND status <> 'deleted'
                 LIMIT 1",
                &[&share_id, &attachment_id],
            )
            .await?;

        row.map(stored_mind_map_share_attachment_from_row).transpose()
    }

    async fn complete_mind_map_share_attachment_upload(
        &self,
        share_id: &str,
        attachment_id: &str,
        update: MindMapShareAttachmentUploadUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_share_attachments
                 SET s3_version_id = $1,
                     checksum_sha256 = $2,
                     status = $3
                 WHERE share_id = $4 AND id = $5 AND status <> 'deleted'",
                &[
                    &update.s3_version_id,
                    &update.checksum_sha256,
                    &update.status.as_str(),
                    &share_id,
                    &attachment_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn list_shared_user_groups_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<StoredSharedUserGroup>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, owner_user_id, owner_username, name, description, members_json, created_at, updated_at
                 FROM user_groups
                 ORDER BY updated_at DESC",
                &[],
            )
            .await?;

        let groups = rows
            .into_iter()
            .map(stored_shared_user_group_from_row)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(groups
            .into_iter()
            .filter(|group| group.owner_user_id == user_id || group.members.iter().any(|member| member.user_id == user_id))
            .collect())
    }

    async fn create_shared_user_group(&self, group: NewSharedUserGroup) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO user_groups (
                    id, owner_user_id, owner_username, name, description, members_json, created_at, updated_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8
                 )",
                &[
                    &group.id,
                    &group.owner_user_id,
                    &group.owner_username,
                    &group.name,
                    &group.description,
                    &Json(&group.members),
                    &group.created_at,
                    &group.updated_at,
                ],
            )
            .await?;

        Ok(())
    }

    async fn get_shared_user_group(&self, id: &str) -> Result<Option<StoredSharedUserGroup>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT id, owner_user_id, owner_username, name, description, members_json, created_at, updated_at
                 FROM user_groups
                 WHERE id = $1
                 LIMIT 1",
                &[&id],
            )
            .await?;

        row.map(stored_shared_user_group_from_row).transpose()
    }

    async fn update_shared_user_group(
        &self,
        id: &str,
        owner_user_id: &str,
        update: SharedUserGroupUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE user_groups
                 SET name = $1,
                     description = $2,
                     members_json = $3,
                     updated_at = $4
                 WHERE id = $5 AND owner_user_id = $6",
                &[
                    &update.name,
                    &update.description,
                    &Json(&update.members),
                    &update.updated_at,
                    &id,
                    &owner_user_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn delete_shared_user_group(&self, id: &str, owner_user_id: &str) -> Result<(), AppError> {
        self.client
            .execute(
                "DELETE FROM user_groups WHERE id = $1 AND owner_user_id = $2",
                &[&id, &owner_user_id],
            )
            .await?;

        Ok(())
    }

    async fn list_plaintext_maps_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<StoredPlainTextMap>, AppError> {
        let groups = self.list_shared_user_groups_for_user(user_id).await?;
        let rows = self
            .client
            .query(
                "SELECT id, owner_user_id, owner_username, title, summary, content_json,
                        direct_user_shares_json, group_shares_json, created_at, updated_at
                 FROM plaintext_maps
                 ORDER BY updated_at DESC",
                &[],
            )
            .await?;

        let maps = rows
            .into_iter()
            .map(stored_plaintext_map_from_row)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(maps
            .into_iter()
            .filter(|map| plaintext_map_visible_to_user(map, user_id, &groups))
            .collect())
    }

    async fn create_plaintext_map(&self, map: NewPlainTextMap) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO plaintext_maps (
                    id, owner_user_id, owner_username, title, summary, content_json,
                    direct_user_shares_json, group_shares_json, created_at, updated_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6,
                    $7, $8, $9, $10
                 )",
                &[
                    &map.id,
                    &map.owner_user_id,
                    &map.owner_username,
                    &map.title,
                    &map.summary,
                    &Json(&map.content_json),
                    &Json(&map.direct_user_shares),
                    &Json(&map.group_shares),
                    &map.created_at,
                    &map.updated_at,
                ],
            )
            .await?;

        Ok(())
    }

    async fn get_plaintext_map(&self, id: &str) -> Result<Option<StoredPlainTextMap>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT id, owner_user_id, owner_username, title, summary, content_json,
                        direct_user_shares_json, group_shares_json, created_at, updated_at
                 FROM plaintext_maps
                 WHERE id = $1
                 LIMIT 1",
                &[&id],
            )
            .await?;

        row.map(stored_plaintext_map_from_row).transpose()
    }

    async fn update_plaintext_map(
        &self,
        id: &str,
        update: PlainTextMapUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE plaintext_maps
                 SET title = $1,
                     summary = $2,
                     content_json = $3,
                     direct_user_shares_json = $4,
                     group_shares_json = $5,
                     updated_at = $6
                 WHERE id = $7",
                &[
                    &update.title,
                    &update.summary,
                    &Json(&update.content_json),
                    &Json(&update.direct_user_shares),
                    &Json(&update.group_shares),
                    &update.updated_at,
                    &id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn delete_plaintext_map(&self, id: &str, owner_user_id: &str) -> Result<(), AppError> {
        self.client
            .execute(
                "DELETE FROM plaintext_maps WHERE id = $1 AND owner_user_id = $2",
                &[&id, &owner_user_id],
            )
            .await?;

        Ok(())
    }
}

fn stored_user_from_row(row: Row) -> Result<StoredUser, AppError> {
    let subscription_tier_raw: String = row.get("subscription_tier");

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
        subscription_tier: parse_subscription_tier(&subscription_tier_raw),
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

fn admin_feedback_from_row(row: Row) -> Result<AdminFeedbackRecord, AppError> {
    Ok(AdminFeedbackRecord {
        public_id: row.get("id"),
        name: row.get("name"),
        email: row.get("email"),
        subject: row.get("subject"),
        message: row.get("message"),
        page_url: row.get("page_url"),
        created_at: row.get("created_at"),
        is_archived: row.get("is_archived"),
        archived_at: row.get("archived_at"),
    })
}

fn admin_audit_from_row(row: Row) -> Result<AdminAuditEvent, AppError> {
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

fn admin_user_from_row(row: Row) -> Result<AdminUserRecord, AppError> {
    let subscription_tier_raw: String = row.get("subscription_tier");

    Ok(AdminUserRecord {
        id: row.get("id"),
        username: row.get("username"),
        created_at: row.get("created_at"),
        subscription_tier: parse_subscription_tier(&subscription_tier_raw),
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

fn stored_mind_map_from_row(row: Row) -> Result<StoredMindMap, AppError> {
    Ok(StoredMindMap {
        id: row.get("id"),
        user_id: row.get("user_id"),
        title_encrypted: row.get("title_encrypted"),
        minio_object_key: row.get("minio_object_key"),
        eph_classical_public: row.get("eph_classical_public"),
        eph_pq_ciphertext: row.get("eph_pq_ciphertext"),
        wrapped_dek: row.get("wrapped_dek"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        minio_version_id: row.get("minio_version_id"),
        version_history: row.get::<_, Json<Vec<VersionSnapshot>>>("version_history").0,
        vault_color: row.get("vault_color"),
        vault_note_encrypted: row.get("vault_note_encrypted"),
        vault_sharing_mode: row.get("vault_sharing_mode"),
        vault_encryption_mode: row.get("vault_encryption_mode"),
        max_versions: row.get::<_, i32>("max_versions") as u32,
        vault_labels: row.get::<_, Json<Vec<String>>>("vault_labels").0,
    })
}

fn stored_shared_user_group_from_row(row: Row) -> Result<StoredSharedUserGroup, AppError> {
    Ok(StoredSharedUserGroup {
        id: row.get("id"),
        owner_user_id: row.get("owner_user_id"),
        owner_username: row.get("owner_username"),
        name: row.get("name"),
        description: row.get("description"),
        members: row.get::<_, Json<Vec<GroupMember>>>("members_json").0,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn stored_plaintext_map_from_row(row: Row) -> Result<StoredPlainTextMap, AppError> {
    Ok(StoredPlainTextMap {
        id: row.get("id"),
        owner_user_id: row.get("owner_user_id"),
        owner_username: row.get("owner_username"),
        title: row.get("title"),
        summary: row.get("summary"),
        content_json: row.get::<_, Json<serde_json::Value>>("content_json").0,
        direct_user_shares: row.get::<_, Json<Vec<DirectUserShare>>>("direct_user_shares_json").0,
        group_shares: row.get::<_, Json<Vec<GroupShare>>>("group_shares_json").0,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn plaintext_map_visible_to_user(
    map: &StoredPlainTextMap,
    user_id: &str,
    groups: &[StoredSharedUserGroup],
) -> bool {
    if map.owner_user_id == user_id {
        return true;
    }

    if map.direct_user_shares.iter().any(|share| share.user_id == user_id) {
        return true;
    }

    map.group_shares.iter().any(|share| {
        groups.iter().any(|group| {
            group.id == share.group_id
                && (group.owner_user_id == user_id || group.members.iter().any(|member| member.user_id == user_id))
        })
    })
}

fn parse_subscription_tier(value: &str) -> SubscriptionTier {
    SubscriptionTier::from_str(value)
}

fn stored_mind_map_attachment_from_row(row: Row) -> Result<StoredMindMapAttachment, AppError> {
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

fn stored_mind_map_share_from_row(row: Row) -> Result<StoredMindMapShare, AppError> {
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

fn stored_mind_map_share_attachment_from_row(
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

fn user_account_settings_from_row(row: Row) -> Result<UserAccountSettings, AppError> {
    Ok(UserAccountSettings {
        locale: row.get("locale"),
        timezone: row.get("timezone"),
        date_format: row.get("date_format"),
        accessibility_reduce_motion: row.get("accessibility_reduce_motion"),
        sync_appearance_across_devices: row.get("sync_appearance_across_devices"),
        default_share_expiry_days: row.get("default_share_expiry_days"),
        default_include_attachments_on_share: row.get("default_include_attachments_on_share"),
        default_map_layout: row.get("default_map_layout"),
        default_map_theme: row.get("default_map_theme"),
        default_export_format: row.get("default_export_format"),
        default_node_style_preset: row.get("default_node_style_preset"),
        user_labels_json: row.try_get("user_labels_json").unwrap_or_else(|_| "[]".to_string()),
        updated_at: row.get("updated_at"),
    })
}

fn user_notification_settings_from_row(row: Row) -> Result<UserNotificationSettings, AppError> {
    Ok(UserNotificationSettings {
        inbox_enabled: row.get("inbox_enabled"),
        email_enabled: row.get("email_enabled"),
        push_enabled: row.get("push_enabled"),
        desktop_enabled: row.get("desktop_enabled"),
        digest_enabled: row.get("digest_enabled"),
        quiet_hours_start: row.get("quiet_hours_start"),
        quiet_hours_end: row.get("quiet_hours_end"),
        allow_preview_local_only: row.get("allow_preview_local_only"),
        share_created: row.get("share_created"),
        share_revoked: row.get("share_revoked"),
        attachment_upload_failures: row.get("attachment_upload_failures"),
        billing_notices: row.get("billing_notices"),
        security_alerts: row.get("security_alerts"),
        admin_messages: row.get("admin_messages"),
        collaboration_mentions: row.get("collaboration_mentions"),
        updated_at: row.get("updated_at"),
    })
}

fn notification_event_from_row(row: Row) -> Result<StoredNotificationEvent, AppError> {
    let priority_raw: String = row.get("priority");

    Ok(StoredNotificationEvent {
        id: row.get("id"),
        user_id: row.get("user_id"),
        event_type: row.get("event_type"),
        category: row.get("category"),
        priority: NotificationPriority::from_str(&priority_raw),
        actor_user_id: row.get("actor_user_id"),
        object_type: row.get("object_type"),
        object_id: row.get("object_id"),
        object_label_safe: row.get("object_label_safe"),
        reason_code: row.get("reason_code"),
        payload_json: row.get::<_, Json<serde_json::Value>>("payload_json").0,
        created_at: row.get("created_at"),
        read_at: row.get("read_at"),
        saved_at: row.get("saved_at"),
        done_at: row.get("done_at"),
    })
}