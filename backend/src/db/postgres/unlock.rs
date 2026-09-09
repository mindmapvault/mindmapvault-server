use async_trait::async_trait;
use chrono::Utc;

use crate::{
    db::{postgres::PostgresDb, sql_store::UnlockStore},
    error::AppError,
    models::unlock::{UnlockKind, UnlockMethod},
};

const COLUMNS: &str =
    "id, user_id, kind, label, wrapped_master_key, created_at, last_used_at";

fn from_row(row: &tokio_postgres::Row) -> Option<UnlockMethod> {
    let kind: String = row.get(2);
    Some(UnlockMethod {
        id: row.get(0),
        user_id: row.get(1),
        // A row whose kind this build does not understand is skipped rather
        // than guessed at — a downgrade must not misread a newer method.
        kind: UnlockKind::parse(&kind)?,
        label: row.get(3),
        wrapped_master_key: row.get(4),
        created_at: row.get(5),
        last_used_at: row.get(6),
    })
}

#[async_trait]
impl UnlockStore for PostgresDb {
    async fn list_unlock_methods(&self, user_id: &str) -> Result<Vec<UnlockMethod>, AppError> {
        let rows = self
            .client
            .query(
                &format!(
                    "SELECT {COLUMNS} FROM unlock_methods
                     WHERE user_id = $1 ORDER BY created_at"
                ),
                &[&user_id],
            )
            .await?;
        Ok(rows.iter().filter_map(from_row).collect())
    }

    async fn load_unlock_method(
        &self,
        user_id: &str,
        id: &str,
    ) -> Result<Option<UnlockMethod>, AppError> {
        // Scoped to the user as well as the id: an id alone must never reach
        // somebody else's wrapped key, even though they could not decrypt it.
        let row = self
            .client
            .query_opt(
                &format!("SELECT {COLUMNS} FROM unlock_methods WHERE user_id = $1 AND id = $2"),
                &[&user_id, &id],
            )
            .await?;
        Ok(row.as_ref().and_then(from_row))
    }

    async fn save_unlock_method(&self, method: &UnlockMethod) -> Result<bool, AppError> {
        // The user_id guard stops one account overwriting another's row. When it
        // does not match, no row is written — and returning Ok(()) there would
        // have told a device it was trusted when the server had stored nothing.
        let affected = self
            .client
            .execute(
                "INSERT INTO unlock_methods
                     (id, user_id, kind, label, wrapped_master_key, created_at)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (id) DO UPDATE SET
                    label = EXCLUDED.label,
                    wrapped_master_key = EXCLUDED.wrapped_master_key
                 WHERE unlock_methods.user_id = EXCLUDED.user_id",
                &[
                    &method.id,
                    &method.user_id,
                    &method.kind.as_str(),
                    &method.label,
                    &method.wrapped_master_key,
                    &method.created_at,
                ],
            )
            .await?;
        Ok(affected > 0)
    }

    async fn touch_unlock_method(&self, user_id: &str, id: &str) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE unlock_methods SET last_used_at = $3 WHERE user_id = $1 AND id = $2",
                &[&user_id, &id, &Utc::now()],
            )
            .await?;
        Ok(())
    }

    async fn delete_unlock_method(&self, user_id: &str, id: &str) -> Result<bool, AppError> {
        let affected = self
            .client
            .execute(
                "DELETE FROM unlock_methods WHERE user_id = $1 AND id = $2",
                &[&user_id, &id],
            )
            .await?;
        Ok(affected > 0)
    }

    async fn delete_all_unlock_methods(&self, user_id: &str) -> Result<u64, AppError> {
        // Called when the master key changes. Every stored copy was encrypted
        // under the old one, so they are now undecryptable noise — leaving them
        // would mean a trusted device silently failing to unlock with no way to
        // tell why.
        let affected = self
            .client
            .execute("DELETE FROM unlock_methods WHERE user_id = $1", &[&user_id])
            .await?;
        Ok(affected)
    }
}
