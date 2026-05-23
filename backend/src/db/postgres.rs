use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use tokio_postgres::{types::Json, Client, NoTls, Row};

use crate::{
    config::AppConfig,
    db::sql_store::{
        AdminUserAdminUpdate, AdminUserRecord, ManualSubscriptionUpdate,
        MindMapAttachmentUploadUpdate, MindMapContentUpdate, MindMapMetaUpdate,
        NewMindMap,
        NewMindMapAttachment, NewUser,
        RotateCredentialsUpdate,
        SqlStore,
        StoredMindMap, StoredMindMapAttachment,
        StoredUser,
        UserProfileUpdate,
    },
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
                    default_map_layout TEXT NOT NULL DEFAULT 'mindmap',
                    default_map_theme TEXT NOT NULL DEFAULT 'system',
                    default_export_format TEXT NOT NULL DEFAULT 'cryptmind',
                    default_node_style_preset TEXT NOT NULL DEFAULT 'default',
                    user_labels_json TEXT NOT NULL DEFAULT '[]',
                    updated_at TIMESTAMPTZ NOT NULL
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
                    vault_encryption_mode TEXT NOT NULL DEFAULT 'standard',
                    max_versions INTEGER NOT NULL,
                    vault_labels JSONB NOT NULL DEFAULT '[]'::jsonb
                );

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
                CREATE INDEX IF NOT EXISTS idx_admin_audit_events_created_at ON admin_audit_events (created_at DESC);",
            )
            .await
            .context("failed to ensure PostgreSQL schema")?;

        Ok(())
    }
}

#[async_trait]
impl SqlStore for PostgresDb {
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
                        sync_appearance_across_devices, default_map_layout,
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
                    sync_appearance_across_devices, default_map_layout,
                    default_map_theme, default_export_format, default_node_style_preset,
                    user_labels_json, updated_at
                 ) VALUES (
                    $1, $2, $3, $4, $5,
                    $6, $7, $8, $9,
                    $10, $11, $12
                 )
                 ON CONFLICT (user_id) DO UPDATE SET
                    locale = EXCLUDED.locale,
                    timezone = EXCLUDED.timezone,
                    date_format = EXCLUDED.date_format,
                    accessibility_reduce_motion = EXCLUDED.accessibility_reduce_motion,
                    sync_appearance_across_devices = EXCLUDED.sync_appearance_across_devices,
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

    async fn list_mind_maps(&self, user_id: &str) -> Result<Vec<StoredMindMap>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT
                    id, user_id, title_encrypted, minio_object_key, eph_classical_public,
                    eph_pq_ciphertext, wrapped_dek, created_at, updated_at, minio_version_id,
                    version_history, vault_color, vault_note_encrypted,
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
                    version_history, vault_color, vault_note_encrypted,
                    vault_encryption_mode, max_versions, vault_labels
                 ) VALUES (
                    $1, $2, $3, $4, $5,
                    $6, $7, $8, $9, $10,
                    $11, $12, $13,
                    $14, $15, $16
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
                    version_history, vault_color, vault_note_encrypted,
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
                     vault_encryption_mode = $3,
                     max_versions = $4,
                     title_encrypted = $5,
                     updated_at = $6,
                     vault_labels = $7
                 WHERE id = $8 AND user_id = $9",
                &[
                    &update.vault_color,
                    &update.vault_note_encrypted,
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
}

fn stored_user_from_row(row: Row) -> Result<StoredUser, AppError> {
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
        vault_encryption_mode: row.get("vault_encryption_mode"),
        max_versions: row.get::<_, i32>("max_versions") as u32,
        vault_labels: row.get::<_, Json<Vec<String>>>("vault_labels").0,
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

fn user_account_settings_from_row(row: Row) -> Result<UserAccountSettings, AppError> {
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
