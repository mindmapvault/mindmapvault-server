//! Registration invites.
//!
//! One trait's worth of queries. See `sql_store` for why they are split.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::{
    db::sql_store::InviteStore,
    error::AppError,
    models::invite::RegistrationInvite,
};

use super::row::*;
use super::PostgresDb;

#[async_trait]
impl InviteStore for PostgresDb {
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
}
