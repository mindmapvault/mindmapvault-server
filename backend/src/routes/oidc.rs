//! The federated sign-in flow.
//!
//! Four routes:
//!
//! * `GET  /auth/oidc/providers`      — what buttons the sign-in page draws
//! * `GET  /auth/oidc/{id}/start`     — sends the browser to the provider
//! * `GET  /auth/oidc/{id}/callback`  — brings it back, and issues our tokens
//! * `POST /auth/oidc/enrol`          — finishes a new account
//!
//! The provider tells us *who* someone is. It cannot tell us what decrypts
//! their vaults: the master key is derived from a passphrase this server never
//! sees. So a brand new federated account is created with **no username of its
//! own and no key material**, and is useless until the client enrols — which is
//! where the user picks both. Until then the account has an empty `argon2_salt`,
//! which `/auth/salt` and `/auth/login` both already treat as "no password
//! credential".

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    db::sql_store::{DynSqlStore, NewUser},
    error::AppError,
    middleware::{
        auth::{AuthenticatedUser, JwtService, KeyVersionCache},
        client_ip::ClientIp,
        oidc_flow::{pkce_challenge, random_token, OidcFlows},
        throttle::{AttemptClass, AuthThrottle},
    },
    models::{
        instance_settings::InstanceSettingsHandle,
        oidc::{
            EnrolKeysRequest, FederatedIdentity, IdTokenClaims, OidcProvider, PublicOidcProvider,
        },
        user::{Argon2Params, SubscriptionTier},
    },
    routes::oidc_client::{discover, exchange_and_verify, OidcCache},
};

#[derive(Clone)]
pub struct OidcState {
    pub db: DynSqlStore,
    pub jwt: Arc<JwtService>,
    pub settings: InstanceSettingsHandle,
    pub throttle: Arc<AuthThrottle>,
    pub key_versions: KeyVersionCache,
    pub flows: Arc<OidcFlows>,
    pub cache: Arc<OidcCache>,
    /// The origin this server is reached on, e.g. `https://maps.example.com`.
    ///
    /// Must be configured rather than taken from the request: the redirect URI
    /// has to match what is registered at the provider exactly, and a `Host`
    /// header is chosen by the caller.
    pub public_base_url: String,
}

impl axum::extract::FromRef<OidcState> for InstanceSettingsHandle {
    fn from_ref(state: &OidcState) -> Self {
        state.settings.clone()
    }
}

impl axum::extract::FromRef<OidcState> for Arc<JwtService> {
    fn from_ref(state: &OidcState) -> Self {
        state.jwt.clone()
    }
}

impl axum::extract::FromRef<OidcState> for DynSqlStore {
    fn from_ref(state: &OidcState) -> Self {
        state.db.clone()
    }
}

impl axum::extract::FromRef<OidcState> for KeyVersionCache {
    fn from_ref(state: &OidcState) -> Self {
        state.key_versions.clone()
    }
}

pub fn router(state: OidcState) -> Router {
    Router::new()
        .route("/providers", get(list_providers))
        .route("/{id}/start", get(start))
        .route("/{id}/callback", get(callback))
        .route("/enrol", post(enrol))
        .with_state(state)
}

async fn list_providers(
    State(state): State<OidcState>,
) -> Result<Json<Vec<PublicOidcProvider>>, AppError> {
    let providers = state.db.list_oidc_providers().await?;
    Ok(Json(
        providers
            .iter()
            .filter(|provider| provider.enabled)
            .map(PublicOidcProvider::from)
            .collect(),
    ))
}

fn redirect_uri(state: &OidcState, provider_id: &str) -> String {
    format!(
        "{}/api/auth/oidc/{}/callback",
        state.public_base_url.trim_end_matches('/'),
        provider_id
    )
}

async fn enabled_provider(state: &OidcState, id: &str) -> Result<OidcProvider, AppError> {
    let provider = state
        .db
        .load_oidc_provider(id)
        .await?
        .filter(|provider| provider.enabled)
        .ok_or_else(|| AppError::NotFound("no such sign-in provider".to_string()))?;

    if state.public_base_url.trim().is_empty() {
        return Err(AppError::Internal(
            "PUBLIC_BASE_URL is not set, so the redirect back from the provider cannot be built"
                .to_string(),
        ));
    }
    Ok(provider)
}

async fn start(
    State(state): State<OidcState>,
    client_ip: ClientIp,
    Path(id): Path<String>,
) -> Result<Redirect, AppError> {
    // Starting a sign-in is a credential attempt: it costs a round trip to the
    // provider and can end in an account being created.
    let limit = state.settings.get().auth_rate_limit_per_minute;
    state
        .throttle
        .check_address(client_ip.0, AttemptClass::Credential, limit)
        .map_err(|retry_after| {
            AppError::TooManyRequests(
                "too many attempts; please wait and try again".to_string(),
                retry_after.as_secs().max(1),
            )
        })?;

    let provider = enabled_provider(&state, &id).await?;
    let discovery = discover(&state.cache, &provider).await?;

    let nonce = random_token();
    let verifier = random_token();
    let challenge = pkce_challenge(&verifier);
    let state_token = state
        .flows
        .begin(&provider.id, nonce.clone(), verifier, "/vaults".to_string());

    let url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&nonce={}\
         &code_challenge={}&code_challenge_method=S256",
        discovery.authorization_endpoint,
        urlencoding::encode(&provider.client_id),
        urlencoding::encode(&redirect_uri(&state, &provider.id)),
        urlencoding::encode(&provider.effective_scopes()),
        urlencoding::encode(&state_token),
        urlencoding::encode(&nonce),
        urlencoding::encode(&challenge),
    );

    Ok(Redirect::temporary(&url))
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn callback(
    State(state): State<OidcState>,
    Path(id): Path<String>,
    Query(query): Query<CallbackQuery>,
) -> Result<impl IntoResponse, AppError> {
    // The provider refused, or the user declined consent. Send them back to the
    // sign-in page with something readable rather than a blank screen.
    if let Some(error) = query.error {
        let detail = query.error_description.unwrap_or_else(|| error.clone());
        tracing::info!(provider = %id, %error, %detail, "federated sign-in was refused");
        return Ok(Redirect::temporary(&format!(
            "/login?sso_error={}",
            urlencoding::encode(&detail)
        )));
    }

    let (code, state_token) = match (query.code, query.state) {
        (Some(code), Some(state_token)) => (code, state_token),
        _ => return Err(AppError::BadRequest("incomplete callback".to_string())),
    };

    // Consuming the flow proves this callback belongs to a sign-in we started,
    // and that it has not already been used.
    let flow = state
        .flows
        .take(&state_token)
        .ok_or_else(|| AppError::Unauthorized("this sign-in has expired; please try again".to_string()))?;

    if flow.provider_id != id {
        return Err(AppError::Unauthorized(
            "this sign-in was started with a different provider".to_string(),
        ));
    }

    let provider = enabled_provider(&state, &id).await?;
    let discovery = discover(&state.cache, &provider).await?;
    let claims = exchange_and_verify(
        &state.cache,
        &provider,
        &discovery,
        &code,
        &flow.pkce_verifier,
        &redirect_uri(&state, &provider.id),
        &flow.nonce,
    )
    .await?;

    let (user_id, key_version, needs_enrolment) = resolve_account(&state, &provider, &claims).await?;

    let access = state.jwt.issue_access_token(&user_id, key_version)?;
    let refresh = state.jwt.issue_refresh_token(&user_id, key_version)?;
    state.key_versions.set(&user_id, key_version);

    // Tokens ride in the fragment, which browsers do not send to servers and
    // which stays out of access logs and `Referer` headers. The app reads them
    // and clears the hash.
    let target = format!(
        "/auth/callback#access_token={}&refresh_token={}&enrol={}",
        urlencoding::encode(&access),
        urlencoding::encode(&refresh),
        if needs_enrolment { "1" } else { "0" },
    );
    Ok(Redirect::temporary(&target))
}

/// Finds the account this subject belongs to, creating a shell one if the
/// subject is new and the instance is open to sign-ups.
///
/// Returns whether the account still needs enrolment.
async fn resolve_account(
    state: &OidcState,
    provider: &OidcProvider,
    claims: &IdTokenClaims,
) -> Result<(String, u32, bool), AppError> {
    if let Some(user) = state
        .db
        .load_user_by_federated_identity(&provider.id, &claims.sub)
        .await?
    {
        if user.is_locked {
            return Err(AppError::Unauthorized("account is locked".to_string()));
        }
        // An account whose enrolment never finished has no keys yet, and must
        // be sent back to finish rather than into an editor with nothing to
        // decrypt.
        let needs_enrolment = user.argon2_salt.is_empty();
        return Ok((user.id, user.key_version, needs_enrolment));
    }

    if !state.settings.get().registration_enabled {
        return Err(AppError::Forbidden(
            "registration is closed on this server".to_string(),
        ));
    }

    // A shell account: no username of its own yet, and no key material. The
    // placeholder is unique and obviously provisional, and is replaced by the
    // name the user picks at enrolment.
    let user_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    state
        .db
        .create_user(NewUser {
            id: user_id.clone(),
            username: format!("pending-{user_id}"),
            // Both empty: this account has no password credential, which is
            // exactly what /auth/login and /auth/salt already test for.
            auth_hash: String::new(),
            argon2_salt: String::new(),
            argon2_params: Argon2Params::default(),
            classical_public_key: String::new(),
            pq_public_key: String::new(),
            classical_priv_encrypted: String::new(),
            pq_priv_encrypted: String::new(),
            key_version: 1,
            created_at: now,
            subscription_tier: SubscriptionTier::Free,
            stripe_customer_id: None,
            stripe_subscription_id: None,
            stripe_subscription_status: None,
            subscription_current_period_end: None,
            first_name: None,
            last_name: None,
            // Recorded for display only. It is never what an account is looked
            // up by — see the note on FederatedIdentity.
            email: verified_email(claims),
            is_locked: false,
            locked_reason: None,
            admin_note: None,
            manual_subscription_tier: None,
            manual_subscription_expires_at: None,
            manual_subscription_reason: None,
            manual_subscription_granted_by: None,
            access_grants: Vec::new(),
        })
        .await?;

    state
        .db
        .link_federated_identity(&FederatedIdentity {
            provider_id: provider.id.clone(),
            subject: claims.sub.clone(),
            user_id: user_id.clone(),
            created_at: now,
        })
        .await?;

    tracing::info!(provider = %provider.id, user_id = %user_id, "federated account created");
    Ok((user_id, 1, true))
}

/// The email claim, but only when the provider says it checked it.
///
/// An unverified address is a string the user typed at the provider, and
/// storing it as though it were confirmed invites someone to match on it later.
fn verified_email(claims: &IdTokenClaims) -> Option<String> {
    match (claims.email.as_deref(), claims.email_verified) {
        (Some(email), Some(true)) => Some(email.to_string()),
        _ => None,
    }
}

#[derive(Debug, Serialize)]
struct EnrolResponse {
    username: String,
}

/// Finishes a federated account: the username the user chose, and the key
/// material their vault passphrase produced.
async fn enrol(
    State(state): State<OidcState>,
    user: AuthenticatedUser,
    Json(body): Json<EnrolKeysRequest>,
) -> Result<Json<EnrolResponse>, AppError> {
    let username = normalize_username(&body.username)?;

    if body.argon2_salt.trim().is_empty()
        || body.classical_public_key.trim().is_empty()
        || body.pq_public_key.trim().is_empty()
        || body.classical_priv_encrypted.trim().is_empty()
        || body.pq_priv_encrypted.trim().is_empty()
    {
        return Err(AppError::BadRequest(
            "the key bundle is incomplete".to_string(),
        ));
    }

    if state.db.load_user_by_username(&username).await?.is_some() {
        return Err(AppError::Conflict("that username is taken".to_string()));
    }

    let request = EnrolKeysRequest { username: username.clone(), ..body };

    // False means the account already had a salt. Enrolment runs once; a second
    // one would replace the keys every existing vault is encrypted under.
    if !state.db.enrol_account_keys(&user.0, &request).await? {
        return Err(AppError::Conflict(
            "this account has already been set up".to_string(),
        ));
    }

    tracing::info!(user_id = %user.0, "federated account enrolled");
    Ok(Json(EnrolResponse { username }))
}

fn normalize_username(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("a username is required".to_string()));
    }
    if trimmed.len() > 64 {
        return Err(AppError::BadRequest("that username is too long".to_string()));
    }
    // `pending-` is how an unenrolled account is spelled, so it cannot also be
    // something a user picks.
    if trimmed.to_lowercase().starts_with("pending-") {
        return Err(AppError::BadRequest(
            "that username is reserved".to_string(),
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(AppError::BadRequest(
            "a username may use letters, digits, dot, dash and underscore".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(email: Option<&str>, verified: Option<bool>) -> IdTokenClaims {
        IdTokenClaims {
            sub: "subject".into(),
            iss: "https://idp.example".into(),
            aud: serde_json::Value::String("client".into()),
            exp: 0,
            nonce: None,
            email: email.map(str::to_string),
            email_verified: verified,
            preferred_username: None,
            name: None,
        }
    }

    #[test]
    fn an_unverified_email_is_not_recorded() {
        assert_eq!(verified_email(&claims(Some("a@b.c"), Some(false))), None);
        // A provider that says nothing has not checked it either.
        assert_eq!(verified_email(&claims(Some("a@b.c"), None)), None);
    }

    #[test]
    fn a_verified_email_is_recorded() {
        assert_eq!(
            verified_email(&claims(Some("a@b.c"), Some(true))),
            Some("a@b.c".to_string())
        );
    }

    #[test]
    fn a_username_is_trimmed_and_kept() {
        assert_eq!(
            normalize_username("  alice.b-1_ ").expect("valid"),
            "alice.b-1_".to_string()
        );
    }

    #[test]
    fn an_empty_or_overlong_username_is_refused() {
        assert!(normalize_username("   ").is_err());
        assert!(normalize_username(&"a".repeat(65)).is_err());
    }

    #[test]
    fn the_placeholder_prefix_cannot_be_claimed() {
        // Otherwise a user could take the name of an account that has not
        // finished enrolling.
        assert!(normalize_username("pending-123").is_err());
        assert!(normalize_username("PENDING-123").is_err());
    }

    #[test]
    fn a_username_may_not_carry_separators_that_read_as_something_else() {
        assert!(normalize_username("alice bob").is_err());
        assert!(normalize_username("alice@example.com").is_err());
        assert!(normalize_username("alice/../root").is_err());
    }
}
