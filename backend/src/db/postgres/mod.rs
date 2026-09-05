//! The PostgreSQL store.
//!
//! The connection and the schema bootstrap live here; the queries are split
//! by domain into the modules below, one per trait in `sql_store`.

mod admin;
mod invite;
mod mind_map;
mod row;
mod system;
mod user;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tokio_postgres::{Client, NoTls};

use crate::{
    config::AppConfig,
    error::AppError,
};

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
