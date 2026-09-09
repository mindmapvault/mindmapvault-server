//! OpenID Connect provider configuration and the in-flight state of a sign-in.
//!
//! Federated sign-in answers *who someone is*. It cannot answer what unlocks
//! their vaults: the master key is `Argon2id(passphrase, salt)` and the server
//! never sees it, so no claim an identity provider can make will produce it.
//!
//! An account created this way therefore has **no password credential** — empty
//! `argon2_salt` and empty `auth_hash`, so `/auth/login` refuses it outright and
//! `/auth/salt` treats it exactly like a username that does not exist. It
//! becomes usable once the client enrols a vault passphrase and uploads the key
//! bundle it derived. See docs/AUTH_HARDENING_PLAN.md.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One configured identity provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcProvider {
    pub id: String,
    /// What the sign-in button says, e.g. "Google" or "Company SSO".
    pub display_name: String,
    /// The issuer URL. Discovery hangs off `{issuer}/.well-known/openid-configuration`.
    pub issuer: String,
    pub client_id: String,
    /// Never leaves the server. Serialised only for the database layer; the
    /// admin API strips it — see `OidcProviderResponse`.
    pub client_secret: String,
    /// Space-separated, always including `openid`.
    pub scopes: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

impl OidcProvider {
    /// Scopes with `openid` guaranteed present, since the flow is meaningless
    /// without it and an operator editing this field will forget.
    pub fn effective_scopes(&self) -> String {
        let mut scopes: Vec<&str> = self.scopes.split_whitespace().collect();
        if !scopes.contains(&"openid") {
            scopes.insert(0, "openid");
        }
        scopes.join(" ")
    }
}

/// What the sign-in page is told: enough to draw a button, and nothing else.
#[derive(Debug, Clone, Serialize)]
pub struct PublicOidcProvider {
    pub id: String,
    pub display_name: String,
}

impl From<&OidcProvider> for PublicOidcProvider {
    fn from(provider: &OidcProvider) -> Self {
        Self {
            id: provider.id.clone(),
            display_name: provider.display_name.clone(),
        }
    }
}

/// What the admin console is told. Deliberately no `client_secret`: it is
/// write-only, so a compromised admin session cannot read back what is already
/// configured.
#[derive(Debug, Clone, Serialize)]
pub struct OidcProviderResponse {
    pub id: String,
    pub display_name: String,
    pub issuer: String,
    pub client_id: String,
    pub scopes: String,
    pub enabled: bool,
    pub has_client_secret: bool,
    pub created_at: DateTime<Utc>,
}

impl From<&OidcProvider> for OidcProviderResponse {
    fn from(provider: &OidcProvider) -> Self {
        Self {
            id: provider.id.clone(),
            display_name: provider.display_name.clone(),
            issuer: provider.issuer.clone(),
            client_id: provider.client_id.clone(),
            scopes: provider.scopes.clone(),
            enabled: provider.enabled,
            has_client_secret: !provider.client_secret.is_empty(),
            created_at: provider.created_at,
        }
    }
}

/// The subset of the discovery document this server uses.
#[derive(Debug, Clone, Deserialize)]
pub struct OidcDiscovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
}

/// The claims we require from an ID token.
#[derive(Debug, Clone, Deserialize)]
pub struct IdTokenClaims {
    /// The provider's stable identifier for this user. This — never the email —
    /// is what an account is linked on: an address can be reassigned to a new
    /// employee, and linking on it would hand them the previous holder's vaults.
    pub sub: String,
    pub iss: String,
    pub aud: serde_json::Value,
    pub exp: i64,
    pub nonce: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub preferred_username: Option<String>,
    pub name: Option<String>,
}

/// A link between a provider's subject and a local account.
#[derive(Debug, Clone)]
pub struct FederatedIdentity {
    pub provider_id: String,
    pub subject: String,
    pub user_id: String,
    pub created_at: DateTime<Utc>,
}

/// What the client sends to finish enrolment: the key material it derived from
/// the vault passphrase the user just chose.
///
/// The passphrase itself never arrives. `auth_hash` is deliberately not part of
/// this — a federated account has no password credential, so there is nothing
/// for `/auth/login` to verify and no counter for an attacker to grind against.
#[derive(Debug, Deserialize)]
pub struct EnrolKeysRequest {
    /// Chosen by the user on the enrolment screen, not taken from the
    /// provider. A name lifted from an IdP claim can collide with an existing
    /// local account, and resolving that collision by merging is an account
    /// takeover. Asking costs one field and cannot collide silently.
    pub username: String,
    pub argon2_salt: String,
    pub argon2_params: crate::models::user::Argon2Params,
    pub classical_public_key: String,
    pub pq_public_key: String,
    pub classical_priv_encrypted: String,
    pub pq_priv_encrypted: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(scopes: &str) -> OidcProvider {
        OidcProvider {
            id: "p1".into(),
            display_name: "Company SSO".into(),
            issuer: "https://idp.example".into(),
            client_id: "client".into(),
            // Distinctive on purpose: asserting on the word "secret" would
            // match the `has_client_secret` field name and pass for the wrong
            // reason.
            client_secret: "sup3rs3kr1t-must-not-leak".into(),
            scopes: scopes.into(),
            enabled: true,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn openid_is_added_when_an_operator_leaves_it_out() {
        assert_eq!(provider("email profile").effective_scopes(), "openid email profile");
    }

    #[test]
    fn openid_is_not_duplicated_when_it_is_already_there() {
        assert_eq!(provider("openid email").effective_scopes(), "openid email");
    }

    #[test]
    fn the_admin_view_never_carries_the_secret() {
        let response = OidcProviderResponse::from(&provider("openid"));
        let json = serde_json::to_string(&response).expect("serialises");
        assert!(!json.contains("sup3rs3kr1t"), "client_secret leaked into: {json}");
        assert!(response.has_client_secret);
    }

    #[test]
    fn the_sign_in_page_is_told_only_what_it_needs_to_draw_a_button() {
        let public = PublicOidcProvider::from(&provider("openid"));
        let json = serde_json::to_string(&public).expect("serialises");
        assert!(!json.contains("idp.example"), "issuer leaked into: {json}");
        assert!(!json.contains("\"client\""), "client_id leaked into: {json}");
    }
}
