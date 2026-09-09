use async_trait::async_trait;

use crate::{
    db::{
        postgres::{row::stored_user_from_row, PostgresDb},
        sql_store::{OidcStore, StoredUser},
    },
    error::AppError,
    models::oidc::{EnrolKeysRequest, FederatedIdentity, OidcProvider},
};

/// The same column list and order `stored_user_from_row` expects. Kept beside
/// it rather than `SELECT u.*`, which would silently shift every field the next
/// time a column is added to the table.
const USER_COLUMNS: &str = "u.id, u.username, u.auth_hash, u.argon2_salt, u.argon2_params,
     u.classical_public_key, u.pq_public_key, u.classical_priv_encrypted, u.pq_priv_encrypted,
     u.key_version, u.created_at, u.subscription_tier, u.stripe_customer_id,
     u.stripe_subscription_id, u.stripe_subscription_status, u.subscription_current_period_end,
     u.first_name, u.last_name, u.email, u.is_locked, u.locked_reason, u.admin_note,
     u.manual_subscription_tier, u.manual_subscription_expires_at, u.manual_subscription_reason,
     u.manual_subscription_granted_by, u.access_grants_json";

const PROVIDER_COLUMNS: &str =
    "id, display_name, issuer, client_id, client_secret, scopes, enabled, created_at";

fn provider_from_row(row: &tokio_postgres::Row) -> OidcProvider {
    OidcProvider {
        id: row.get(0),
        display_name: row.get(1),
        issuer: row.get(2),
        client_id: row.get(3),
        client_secret: row.get(4),
        scopes: row.get(5),
        enabled: row.get(6),
        created_at: row.get(7),
    }
}

#[async_trait]
impl OidcStore for PostgresDb {
    async fn list_oidc_providers(&self) -> Result<Vec<OidcProvider>, AppError> {
        let rows = self
            .client
            .query(
                &format!("SELECT {PROVIDER_COLUMNS} FROM oidc_providers ORDER BY display_name"),
                &[],
            )
            .await?;
        Ok(rows.iter().map(provider_from_row).collect())
    }

    async fn load_oidc_provider(&self, id: &str) -> Result<Option<OidcProvider>, AppError> {
        let row = self
            .client
            .query_opt(
                &format!("SELECT {PROVIDER_COLUMNS} FROM oidc_providers WHERE id = $1"),
                &[&id],
            )
            .await?;
        Ok(row.as_ref().map(provider_from_row))
    }

    async fn upsert_oidc_provider(&self, provider: &OidcProvider) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO oidc_providers (
                    id, display_name, issuer, client_id, client_secret, scopes, enabled, created_at
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (id) DO UPDATE SET
                    display_name = EXCLUDED.display_name,
                    issuer = EXCLUDED.issuer,
                    client_id = EXCLUDED.client_id,
                    -- An empty secret means 'leave the stored one alone', so an
                    -- operator editing the display name does not have to retype
                    -- a credential the console never showed them.
                    client_secret = CASE
                        WHEN EXCLUDED.client_secret = '' THEN oidc_providers.client_secret
                        ELSE EXCLUDED.client_secret
                    END,
                    scopes = EXCLUDED.scopes,
                    enabled = EXCLUDED.enabled",
                &[
                    &provider.id,
                    &provider.display_name,
                    &provider.issuer,
                    &provider.client_id,
                    &provider.client_secret,
                    &provider.scopes,
                    &provider.enabled,
                    &provider.created_at,
                ],
            )
            .await?;
        Ok(())
    }

    async fn delete_oidc_provider(&self, id: &str) -> Result<bool, AppError> {
        let affected = self
            .client
            .execute("DELETE FROM oidc_providers WHERE id = $1", &[&id])
            .await?;
        Ok(affected > 0)
    }

    async fn load_user_by_federated_identity(
        &self,
        provider_id: &str,
        subject: &str,
    ) -> Result<Option<StoredUser>, AppError> {
        let row = self
            .client
            .query_opt(
                &format!(
                    "SELECT {USER_COLUMNS} FROM users u
                     JOIN federated_identities f ON f.user_id = u.id
                     WHERE f.provider_id = $1 AND f.subject = $2"
                ),
                &[&provider_id, &subject],
            )
            .await?;
        row.map(stored_user_from_row).transpose()
    }

    async fn link_federated_identity(&self, identity: &FederatedIdentity) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO federated_identities (provider_id, subject, user_id, created_at)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (provider_id, subject) DO NOTHING",
                &[
                    &identity.provider_id,
                    &identity.subject,
                    &identity.user_id,
                    &identity.created_at,
                ],
            )
            .await?;
        Ok(())
    }

    async fn enrol_account_keys(
        &self,
        user_id: &str,
        keys: &EnrolKeysRequest,
    ) -> Result<bool, AppError> {
        let params = serde_json::to_value(&keys.argon2_params)
            .map_err(|error| AppError::Internal(format!("argon2 params: {error}")))?;

        // The `argon2_salt = ''` guard is the whole safety property: enrolment
        // establishes the keys once. Running it again on an enrolled account
        // would replace the keys every existing vault is encrypted under and
        // orphan the lot.
        let affected = self
            .client
            .execute(
                "UPDATE users SET
                    username = $8,
                    argon2_salt = $2,
                    argon2_params = $3,
                    classical_public_key = $4,
                    pq_public_key = $5,
                    classical_priv_encrypted = $6,
                    pq_priv_encrypted = $7
                 WHERE id = $1 AND argon2_salt = ''",
                &[
                    &user_id,
                    &keys.argon2_salt,
                    &params,
                    &keys.classical_public_key,
                    &keys.pq_public_key,
                    &keys.classical_priv_encrypted,
                    &keys.pq_priv_encrypted,
                    &keys.username,
                ],
            )
            .await?;

        Ok(affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn scopes_are_normalised_before_they_reach_a_provider() {
        let provider = OidcProvider {
            id: "p1".into(),
            display_name: "Keycloak".into(),
            issuer: "https://idp.example/realms/mmv".into(),
            client_id: "mindmapvault".into(),
            client_secret: "shh".into(),
            scopes: "openid email profile".into(),
            enabled: true,
            created_at: Utc::now(),
        };
        assert_eq!(provider.effective_scopes(), "openid email profile");
    }
}
