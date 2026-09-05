//! Health, instance settings, stats and the migration lock.
//!
//! One trait's worth of queries. See `sql_store` for why they are split.

use async_trait::async_trait;

use crate::{
    db::sql_store::SystemStore,
    error::AppError,
    models::{
        instance_settings::InstanceSettings,
        status::DatabaseStats,
    },
};

use super::PostgresDb;

#[async_trait]
impl SystemStore for PostgresDb {
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
}
