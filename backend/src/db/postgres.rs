use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio_postgres::{types::Json, Client, NoTls, Row};

use crate::{
    config::AppConfig,
    db::sql_store::{
        AdminUserAdminUpdate, AdminUserRecord,
        MindMapAttachmentUploadUpdate, MindMapContentUpdate, MindMapMetaUpdate,
        MindMapShareAttachmentUploadUpdate, MindMapShareUploadUpdate,
        NewMindMap,
        NewMindMapAttachment, NewMindMapShare, NewMindMapShareAttachment, NewUser,
        RotateCredentialsUpdate, RotationAttachmentRecord,
        SqlStore,
        StoredMindMap, StoredMindMapAttachment, StoredMindMapShare, StoredMindMapShareAttachment,
        StoredUser,
        UserProfileUpdate,
    },
    error::AppError,
    models::{
        access::UserAccessGrant,
        admin_audit::AdminAuditEvent,
        attachment::AttachmentStatus,
        instance_settings::InstanceSettings,
        invite::RegistrationInvite,
        mindmap::VersionSnapshot,
        settings::UserAccountSettings,
        share::{ShareScope, ShareStatus},
        status::DatabaseStats,
        user::{Argon2Params, SubscriptionTier},
    },
};

#[derive(Clone)]
pub struct PostgresDb {
    client: Arc<Client>,
    /// Kept so short-lived work that must be ISOLATED — the password-rotation
    /// transaction — can open its own connection. On the shared `client`, a
    /// BEGIN/COMMIT pair is not a private transaction: any other handler's
    /// statement awaited in between executes on the same connection and joins
    /// (or aborts) the transaction.
    dsn: Arc<str>,
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
            dsn: Arc::from(dsn),
        };
        tracing::info!("Ensuring SQL schema");
        tokio::time::timeout(Duration::from_secs(10), crate::db::schema::ensure_schema(&this.client))
            .await
            .context("timed out while ensuring SQL schema")??;
        tracing::info!("Connected to SQL backend");
        Ok(this)
    }

    /// Opens a private connection for work that needs a genuinely isolated
    /// transaction. Everything else in the process shares `self.client` —
    /// one connection — so a BEGIN/COMMIT there is not private: statements
    /// from concurrent handlers land inside the transaction, get rolled back
    /// with it, or abort it. The connection closes when the returned client
    /// is dropped.
    async fn dedicated_client(&self) -> Result<Client, AppError> {
        let (client, connection) = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_postgres::connect(&self.dsn, NoTls),
        )
        .await
        .map_err(|_| AppError::Internal("timed out opening a dedicated SQL connection".to_string()))?
        .map_err(|e| AppError::Internal(format!("failed to open a dedicated SQL connection: {e}")))?;

        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!("dedicated SQL connection closed: {error}");
            }
        });

        Ok(client)
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

    async fn try_lock_migration(&self, key: i64) -> Result<bool, AppError> {
        // A session-level advisory lock on the shared connection: held for as
        // long as this process lives, and dropped by the server if it dies
        // mid-migration, so a crash cannot leave the lock stuck.
        let row = self
            .client
            .query_one("SELECT pg_try_advisory_lock($1)", &[&key])
            .await
            .map_err(AppError::Database)?;
        Ok(row.get::<_, bool>(0))
    }

    async fn unlock_migration(&self, key: i64) -> Result<(), AppError> {
        self.client
            .query_one("SELECT pg_advisory_unlock($1)", &[&key])
            .await
            .map_err(AppError::Database)?;
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

        use std::collections::HashSet;

        // Hash new_auth_token before opening the transaction — the raw token
        // must never rest on disk, and the expensive Argon2id step must not
        // hold the user row lock.
        let salt = SaltString::generate(&mut OsRng);
        let new_auth_hash = Argon2::default()
            .hash_password(update.new_auth_token.as_bytes(), &salt)
            .map_err(|e| AppError::Internal(format!("argon2 hash error: {e}")))?
            .to_string();

        // A dedicated connection, not the shared one: on `self.client`, any
        // concurrent handler's statement would execute inside this
        // transaction — rolled back with it on failure, or aborting it on
        // error. On our own connection the Transaction API also rolls back
        // automatically if we return early and drop it.
        let mut conn = self.dedicated_client().await?;
        let tx = conn
            .transaction()
            .await
            .map_err(|e| AppError::Internal(format!("begin transaction failed: {e}")))?;

        // ── Lock the user row; re-check the version under the lock ────────
        // The row lock serialises rotations against each other; the check
        // catches a rotation that already happened from another session.
        let row = tx
            .query_opt(
                "SELECT key_version FROM users WHERE id = $1 FOR UPDATE",
                &[&user_id],
            )
            .await?
            .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;
        let current_kv = row.get::<_, i32>("key_version") as u32;
        if update.new_key_version != current_kv + 1 {
            return Err(AppError::BadRequest(format!(
                "new_key_version must be {} (current {} + 1)",
                current_kv + 1,
                current_kv,
            )));
        }

        // ── Complete coverage, checked inside the transaction ─────────────
        // Nothing encrypted under the old key may survive the commit, so the
        // submitted sets must exactly equal what the database holds right
        // now — not what it held when the client took its snapshot.
        let vault_rows = tx
            .query(
                "SELECT id,
                        (vault_note_encrypted IS NOT NULL AND vault_note_encrypted <> '') AS has_note
                 FROM mind_maps WHERE user_id = $1",
                &[&user_id],
            )
            .await?;
        let submitted_vaults: HashSet<&str> =
            update.updated_vaults.iter().map(|v| v.id.as_str()).collect();
        let owned_vaults: HashSet<&str> =
            vault_rows.iter().map(|r| r.get::<_, &str>("id")).collect();

        let missing = owned_vaults.difference(&submitted_vaults).count();
        if missing > 0 {
            return Err(AppError::BadRequest(format!(
                "rotation bundle is incomplete — {missing} vault(s) missing",
            )));
        }
        let unknown = submitted_vaults.difference(&owned_vaults).count();
        if unknown > 0 {
            return Err(AppError::BadRequest(format!(
                "rotation bundle references {unknown} unknown vault(s)",
            )));
        }

        // A vault whose note holds ciphertext needs a re-encrypted note in
        // the bundle; `None` means "preserve", which would preserve it under
        // the dead key.
        let notes_by_id: std::collections::HashMap<&str, Option<&str>> = update
            .updated_vaults
            .iter()
            .map(|v| (v.id.as_str(), v.vault_note_encrypted.as_deref()))
            .collect();
        let stranded_notes = vault_rows
            .iter()
            .filter(|r| r.get::<_, bool>("has_note"))
            .filter(|r| {
                !matches!(
                    notes_by_id.get(r.get::<_, &str>("id")),
                    Some(Some(note)) if !note.is_empty()
                )
            })
            .count();
        if stranded_notes > 0 {
            return Err(AppError::BadRequest(format!(
                "rotation bundle leaves {stranded_notes} vault note(s) under the old key",
            )));
        }

        // Same for attachments: every row with a wrapped file key, in any
        // status but `deleted` — pending uploads carry wraps from init time.
        let attachment_rows = tx
            .query(
                "SELECT a.id
                 FROM mind_map_attachments a
                 JOIN mind_maps m ON a.map_id = m.id
                 WHERE m.user_id = $1
                   AND a.status <> 'deleted'
                   AND a.encryption_meta ? 'wrapped_key_b64'",
                &[&user_id],
            )
            .await?;
        let submitted_attachments: HashSet<&str> = update
            .updated_attachments
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        let owned_attachments: HashSet<&str> =
            attachment_rows.iter().map(|r| r.get::<_, &str>("id")).collect();

        let missing = owned_attachments.difference(&submitted_attachments).count();
        if missing > 0 {
            return Err(AppError::BadRequest(format!(
                "rotation bundle is incomplete — {missing} attachment(s) missing",
            )));
        }
        let unknown = submitted_attachments.difference(&owned_attachments).count();
        if unknown > 0 {
            return Err(AppError::BadRequest(format!(
                "rotation bundle references {unknown} unknown attachment(s)",
            )));
        }

        // ── Writes ────────────────────────────────────────────────────────
        tx.execute(
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

        for vault in &update.updated_vaults {
            let note_value: Option<String> = vault
                .vault_note_encrypted
                .as_ref()
                .and_then(|n| if n.is_empty() { None } else { Some(n.clone()) });

            tx.execute(
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

        // Only the wrap changes: merge the two governed fields into the
        // stored metadata, leave format/algorithm/anything else untouched.
        // Every rotated wrap comes out `hkdf-attachment-v1`, which is what
        // retires the legacy `master-aes-256-gcm` wraps.
        for attachment in &update.updated_attachments {
            tx.execute(
                "UPDATE mind_map_attachments AS a
                 SET encryption_meta = a.encryption_meta
                     || jsonb_build_object('wrapped_key_b64', $1::text,
                                           'key_wrap', 'hkdf-attachment-v1')
                 FROM mind_maps AS m
                 WHERE a.id = $2 AND a.map_id = m.id AND m.user_id = $3",
                &[&attachment.wrapped_key_b64, &attachment.id, &user_id],
            )
            .await?;
        }

        tx.commit()
            .await
            .map_err(|e| AppError::Internal(format!("commit failed: {e}")))
    }

    async fn load_user_key_version(&self, user_id: &str) -> Result<Option<u32>, AppError> {
        let row = self
            .client
            .query_opt("SELECT key_version FROM users WHERE id = $1", &[&user_id])
            .await?;
        Ok(row.map(|r| r.get::<_, i32>("key_version") as u32))
    }

    async fn list_rotation_attachments(
        &self,
        user_id: &str,
    ) -> Result<Vec<RotationAttachmentRecord>, AppError> {
        // Must select exactly the rows the rotation transaction will demand
        // coverage of — keep this predicate in lockstep with the one there.
        let rows = self
            .client
            .query(
                "SELECT a.id, a.map_id, a.encryption_meta
                 FROM mind_map_attachments a
                 JOIN mind_maps m ON a.map_id = m.id
                 WHERE m.user_id = $1
                   AND a.status <> 'deleted'
                   AND a.encryption_meta ? 'wrapped_key_b64'
                 ORDER BY a.uploaded_at",
                &[&user_id],
            )
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| RotationAttachmentRecord {
                id: row.get("id"),
                map_id: row.get("map_id"),
                encryption_meta: row.get::<_, Json<serde_json::Value>>("encryption_meta").0,
            })
            .collect())
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

    async fn load_instance_settings(&self) -> Result<Option<InstanceSettings>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT registration_enabled, user_storage_limit_bytes, max_attachment_size_bytes,
                        auth_rate_limit_per_minute, failed_login_threshold,
                        failed_login_lockout_minutes, trust_proxy_headers, updated_at
                 FROM instance_settings
                 WHERE id = 1",
                &[],
            )
            .await?;

        Ok(row.map(|row| InstanceSettings {
            registration_enabled: row.get(0),
            user_storage_limit_bytes: row.get(1),
            max_attachment_size_bytes: row.get(2),
            auth_rate_limit_per_minute: row.get(3),
            failed_login_threshold: row.get(4),
            failed_login_lockout_minutes: row.get(5),
            trust_proxy_headers: row.get(6),
            updated_at: row.get(7),
        }))
    }

    async fn seed_instance_settings(
        &self,
        seed: &InstanceSettings,
    ) -> Result<InstanceSettings, AppError> {
        self.client
            .execute(
                "INSERT INTO instance_settings (
                    id, registration_enabled, user_storage_limit_bytes, max_attachment_size_bytes,
                    auth_rate_limit_per_minute, failed_login_threshold,
                    failed_login_lockout_minutes, trust_proxy_headers, updated_at
                 ) VALUES (1, $1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (id) DO NOTHING",
                &[
                    &seed.registration_enabled,
                    &seed.user_storage_limit_bytes,
                    &seed.max_attachment_size_bytes,
                    &seed.auth_rate_limit_per_minute,
                    &seed.failed_login_threshold,
                    &seed.failed_login_lockout_minutes,
                    &seed.trust_proxy_headers,
                    &seed.updated_at,
                ],
            )
            .await?;

        self.load_instance_settings()
            .await?
            .ok_or_else(|| AppError::Internal("instance settings row missing after seed".to_string()))
    }

    async fn save_instance_settings(
        &self,
        settings: &InstanceSettings,
    ) -> Result<InstanceSettings, AppError> {
        self.client
            .execute(
                "INSERT INTO instance_settings (
                    id, registration_enabled, user_storage_limit_bytes, max_attachment_size_bytes,
                    auth_rate_limit_per_minute, failed_login_threshold,
                    failed_login_lockout_minutes, trust_proxy_headers, updated_at
                 ) VALUES (1, $1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (id) DO UPDATE SET
                    registration_enabled = EXCLUDED.registration_enabled,
                    user_storage_limit_bytes = EXCLUDED.user_storage_limit_bytes,
                    max_attachment_size_bytes = EXCLUDED.max_attachment_size_bytes,
                    auth_rate_limit_per_minute = EXCLUDED.auth_rate_limit_per_minute,
                    failed_login_threshold = EXCLUDED.failed_login_threshold,
                    failed_login_lockout_minutes = EXCLUDED.failed_login_lockout_minutes,
                    trust_proxy_headers = EXCLUDED.trust_proxy_headers,
                    updated_at = EXCLUDED.updated_at",
                &[
                    &settings.registration_enabled,
                    &settings.user_storage_limit_bytes,
                    &settings.max_attachment_size_bytes,
                    &settings.auth_rate_limit_per_minute,
                    &settings.failed_login_threshold,
                    &settings.failed_login_lockout_minutes,
                    &settings.trust_proxy_headers,
                    &settings.updated_at,
                ],
            )
            .await?;

        self.load_instance_settings()
            .await?
            .ok_or_else(|| AppError::Internal("instance settings row missing after save".to_string()))
    }

    async fn sum_user_stored_bytes(&self, user_id: &str) -> Result<i64, AppError> {
        // Everything a user can grow without bound and whose size the database
        // knows. Map blobs live only in object storage and their sizes are not
        // recorded here, so they are outside this total — noted in the admin
        // console next to the limit rather than silently glossed over.
        let row = self
            .client
            .query_one(
                // SUM over BIGINT yields NUMERIC in PostgreSQL, so the total is
                // cast back before it crosses into Rust as an i64.
                "SELECT (
                        COALESCE((
                            SELECT SUM(a.size_bytes)
                            FROM mind_map_attachments a
                            JOIN mind_maps m ON m.id = a.map_id
                            WHERE m.user_id = $1 AND a.status = 'available'
                        ), 0)
                        + COALESCE((
                            SELECT SUM(s.size_bytes)
                            FROM mind_map_shares s
                            JOIN mind_maps m ON m.id = s.map_id
                            WHERE m.user_id = $1 AND s.status = 'available' AND s.revoked = FALSE
                        ), 0)
                        + COALESCE((
                            SELECT SUM(sa.size_bytes)
                            FROM mind_map_share_attachments sa
                            JOIN mind_map_shares s ON s.id = sa.share_id
                            JOIN mind_maps m ON m.id = s.map_id
                            WHERE m.user_id = $1 AND sa.status = 'available' AND s.revoked = FALSE
                        ), 0)
                     )::BIGINT AS total",
                &[&user_id],
            )
            .await?;

        Ok(row.get::<_, i64>(0))
    }

    async fn health_check(&self) -> Result<(), AppError> {
        self.client.query_one("SELECT 1", &[]).await?;
        Ok(())
    }

    async fn database_stats(&self) -> Result<DatabaseStats, AppError> {
        // `server_version` is the short form ("16.4"); `version()` would return
        // a sentence naming the build and platform, which is more than the
        // status page needs and more than is worth showing.
        let row = self
            .client
            .query_one(
                "SELECT current_setting('server_version'), pg_database_size(current_database())",
                &[],
            )
            .await?;

        Ok(DatabaseStats {
            version: row.get::<_, Option<String>>(0),
            size_bytes: row.get::<_, Option<i64>>(1),
        })
    }

    async fn list_registration_invites(&self) -> Result<Vec<RegistrationInvite>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, code, label, created_at, expires_at, used_at, used_by_username
                 FROM registration_invites
                 ORDER BY created_at DESC",
                &[],
            )
            .await?;

        Ok(rows.iter().map(map_registration_invite).collect())
    }

    async fn create_registration_invite(
        &self,
        invite: &RegistrationInvite,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO registration_invites (
                    id, code, label, created_at, expires_at, used_at, used_by_username
                 ) VALUES ($1, $2, $3, $4, $5, NULL, NULL)",
                &[
                    &invite.id,
                    &invite.code,
                    &invite.label,
                    &invite.created_at,
                    &invite.expires_at,
                ],
            )
            .await?;

        Ok(())
    }

    async fn delete_registration_invite(&self, id: &str) -> Result<bool, AppError> {
        let affected = self
            .client
            .execute("DELETE FROM registration_invites WHERE id = $1", &[&id])
            .await?;

        Ok(affected > 0)
    }

    async fn claim_registration_invite(
        &self,
        code: &str,
        username: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<RegistrationInvite>, AppError> {
        let row = self
            .client
            .query_opt(
                "UPDATE registration_invites
                 SET used_at = $3, used_by_username = $2
                 WHERE code = $1
                   AND used_at IS NULL
                   AND (expires_at IS NULL OR expires_at > $3)
                 RETURNING id, code, label, created_at, expires_at, used_at, used_by_username",
                &[&code, &username, &now],
            )
            .await?;

        Ok(row.as_ref().map(map_registration_invite))
    }

    async fn release_registration_invite(&self, id: &str) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE registration_invites
                 SET used_at = NULL, used_by_username = NULL
                 WHERE id = $1",
                &[&id],
            )
            .await?;

        Ok(())
    }

    async fn list_mind_maps(&self, user_id: &str) -> Result<Vec<StoredMindMap>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT
                    id, user_id, title_encrypted, object_key, eph_classical_public,
                    eph_pq_ciphertext, wrapped_dek, created_at, updated_at, current_version_id,
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
                    id, user_id, title_encrypted, object_key, eph_classical_public,
                    eph_pq_ciphertext, wrapped_dek, created_at, updated_at, current_version_id,
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
                    &map.object_key,
                    &map.eph_classical_public,
                    &map.eph_pq_ciphertext,
                    &map.wrapped_dek,
                    &map.created_at,
                    &map.updated_at,
                    &map.current_version_id,
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
                    id, user_id, title_encrypted, object_key, eph_classical_public,
                    eph_pq_ciphertext, wrapped_dek, created_at, updated_at, current_version_id,
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
        current_version_id: &str,
        version_history: Vec<VersionSnapshot>,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_maps
                 SET current_version_id = $1, version_history = $2
                 WHERE id = $3 AND user_id = $4",
                &[&current_version_id, &Json(&version_history), &id, &user_id],
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

    async fn list_purgeable_mind_map_shares(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<StoredMindMapShare>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, map_id, share_name, share_scope, s3_key, s3_version_id, created_by,
                        created_at, updated_at, expires_at, revoked, include_attachments,
                        passphrase_hint, content_type, size_bytes, encryption_meta,
                        checksum_sha256, status
                 FROM mind_map_shares
                 WHERE status = 'available'
                   AND (revoked = TRUE OR (expires_at IS NOT NULL AND expires_at < $1))
                 ORDER BY updated_at ASC
                 LIMIT $2",
                &[&now, &limit],
            )
            .await?;

        rows.into_iter().map(stored_mind_map_share_from_row).collect()
    }

    async fn mark_mind_map_share_purged(&self, share_id: &str) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_shares
                 SET status = 'revoked', revoked = TRUE, size_bytes = 0, updated_at = NOW()
                 WHERE id = $1",
                &[&share_id],
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

fn parse_subscription_tier(value: &str) -> SubscriptionTier {
    SubscriptionTier::from_str(value)
}

fn map_registration_invite(row: &Row) -> RegistrationInvite {
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
