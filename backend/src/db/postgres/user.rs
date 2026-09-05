//! Accounts, keys, per-account settings and credential rotation.
//!
//! One trait's worth of queries. See `sql_store` for why they are split.

use async_trait::async_trait;
use tokio_postgres::types::Json;

use crate::{
    db::sql_store::{
        UserStore,
        AdminUserAdminUpdate, AdminUserRecord, NewUser,
        RotateCredentialsUpdate, RotationAttachmentRecord,
        StoredUser,
        UserProfileUpdate,
    },
    error::AppError,
    models::{
        settings::UserAccountSettings,
        user::SubscriptionTier,
    },
};

use super::row::*;
use super::PostgresDb;

#[async_trait]
impl UserStore for PostgresDb {
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
}
