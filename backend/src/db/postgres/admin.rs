//! The admin audit trail.
//!
//! One trait's worth of queries. See `sql_store` for why they are split.

use async_trait::async_trait;

use crate::{
    db::sql_store::AdminAuditStore,
    error::AppError,
    models::admin_audit::AdminAuditEvent,
};

use super::row::*;
use super::PostgresDb;

#[async_trait]
impl AdminAuditStore for PostgresDb {
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
}
