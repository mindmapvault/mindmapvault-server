//! Talking to an OpenID Connect provider: discovery, code exchange, and
//! verifying the ID token that comes back.
//!
//! The ID token is the only thing here that carries authority, so it is checked
//! rather than trusted:
//!
//! * the **signature**, against a key fetched from the provider's JWKS and
//!   selected by the token's own `kid`;
//! * the **issuer**, against the one the operator configured — a token from
//!   some other provider must not sign anyone in;
//! * the **audience**, against our client id, so a token minted for a different
//!   relying party cannot be replayed at us;
//! * the **nonce**, against the one this server generated for this sign-in,
//!   which is what stops an older token being replayed at all;
//! * the **expiry**, by `jsonwebtoken`.
//!
//! Skip any one of those and the flow becomes a way to sign in as anybody.

use std::{collections::HashMap, sync::Mutex, time::{Duration, Instant}};

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

use crate::{
    error::AppError,
    models::oidc::{IdTokenClaims, OidcDiscovery, OidcProvider},
};

/// How long a discovery document and its JWKS are reused before being fetched
/// again. Short enough to pick up a provider's key rotation without a restart,
/// long enough that a sign-in is not three round trips to the provider.
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);

/// Ceiling on any single call to a provider, so a hanging endpoint cannot hold
/// a request handler open.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Debug, Clone, Deserialize)]
struct Jwk {
    kid: Option<String>,
    kty: String,
    #[serde(default)]
    alg: Option<String>,
    // RSA
    n: Option<String>,
    e: Option<String>,
    // EC
    crv: Option<String>,
    x: Option<String>,
    y: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    id_token: String,
}

#[derive(Clone)]
struct Cached<T> {
    value: T,
    fetched_at: Instant,
}

/// Discovery documents and JWKS, keyed by issuer.
#[derive(Default)]
pub struct OidcCache {
    discovery: Mutex<HashMap<String, Cached<OidcDiscovery>>>,
    jwks: Mutex<HashMap<String, Cached<Jwks>>>,
}

impl OidcCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn discovery(&self, issuer: &str) -> Option<OidcDiscovery> {
        let cache = self.discovery.lock().ok()?;
        let entry = cache.get(issuer)?;
        (entry.fetched_at.elapsed() < CACHE_TTL).then(|| entry.value.clone())
    }

    fn put_discovery(&self, issuer: &str, value: OidcDiscovery) {
        if let Ok(mut cache) = self.discovery.lock() {
            cache.insert(
                issuer.to_string(),
                Cached { value, fetched_at: Instant::now() },
            );
        }
    }

    fn jwks(&self, uri: &str) -> Option<Jwks> {
        let cache = self.jwks.lock().ok()?;
        let entry = cache.get(uri)?;
        (entry.fetched_at.elapsed() < CACHE_TTL).then(|| entry.value.clone())
    }

    fn put_jwks(&self, uri: &str, value: Jwks) {
        if let Ok(mut cache) = self.jwks.lock() {
            cache.insert(uri.to_string(), Cached { value, fetched_at: Instant::now() });
        }
    }
}

fn http() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        // A provider that redirects its discovery endpoint elsewhere is not
        // one we should follow blindly.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| AppError::Internal(format!("http client: {error}")))
}

/// Fetches (or reuses) the provider's discovery document.
pub async fn discover(
    cache: &OidcCache,
    provider: &OidcProvider,
) -> Result<OidcDiscovery, AppError> {
    if let Some(found) = cache.discovery(&provider.issuer) {
        return Ok(found);
    }

    let url = format!(
        "{}/.well-known/openid-configuration",
        provider.issuer.trim_end_matches('/')
    );
    let discovery: OidcDiscovery = http()?
        .get(&url)
        .send()
        .await
        .map_err(|error| AppError::BadRequest(format!("could not reach the identity provider: {error}")))?
        .error_for_status()
        .map_err(|error| AppError::BadRequest(format!("identity provider discovery failed: {error}")))?
        .json()
        .await
        .map_err(|error| AppError::BadRequest(format!("identity provider discovery was not readable: {error}")))?;

    // A document that names a different issuer than the one configured is
    // either a misconfiguration or a substitution; either way it must not be
    // used to sign anybody in.
    if discovery.issuer.trim_end_matches('/') != provider.issuer.trim_end_matches('/') {
        return Err(AppError::BadRequest(format!(
            "identity provider announces issuer {} but is configured as {}",
            discovery.issuer, provider.issuer
        )));
    }

    cache.put_discovery(&provider.issuer, discovery.clone());
    Ok(discovery)
}

/// Exchanges an authorization code for an ID token and verifies it.
///
/// `nonce` is the one this server generated when it sent the user out.
pub async fn exchange_and_verify(
    cache: &OidcCache,
    provider: &OidcProvider,
    discovery: &OidcDiscovery,
    code: &str,
    pkce_verifier: &str,
    redirect_uri: &str,
    nonce: &str,
) -> Result<IdTokenClaims, AppError> {
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", provider.client_id.as_str()),
        ("code_verifier", pkce_verifier),
    ];
    if !provider.client_secret.is_empty() {
        form.push(("client_secret", provider.client_secret.as_str()));
    }

    let response = http()?
        .post(&discovery.token_endpoint)
        .form(&form)
        .send()
        .await
        .map_err(|error| AppError::BadRequest(format!("token exchange failed: {error}")))?;

    if !response.status().is_success() {
        let status = response.status();
        // The body often says exactly what is misconfigured (bad secret, wrong
        // redirect uri), and an operator cannot fix what they cannot see.
        let body = response.text().await.unwrap_or_default();
        tracing::warn!(%status, body = %body, "oidc token exchange rejected");
        return Err(AppError::BadRequest(
            "the identity provider rejected the sign-in".to_string(),
        ));
    }

    let tokens: TokenResponse = response
        .json()
        .await
        .map_err(|error| AppError::BadRequest(format!("token response was not readable: {error}")))?;

    verify_id_token(cache, provider, discovery, &tokens.id_token, nonce).await
}

async fn verify_id_token(
    cache: &OidcCache,
    provider: &OidcProvider,
    discovery: &OidcDiscovery,
    id_token: &str,
    nonce: &str,
) -> Result<IdTokenClaims, AppError> {
    let header = decode_header(id_token)
        .map_err(|error| AppError::BadRequest(format!("id token header unreadable: {error}")))?;

    let jwks = match cache.jwks(&discovery.jwks_uri) {
        Some(found) => found,
        None => {
            let fetched: Jwks = http()?
                .get(&discovery.jwks_uri)
                .send()
                .await
                .map_err(|error| AppError::BadRequest(format!("could not fetch signing keys: {error}")))?
                .json()
                .await
                .map_err(|error| AppError::BadRequest(format!("signing keys unreadable: {error}")))?;
            cache.put_jwks(&discovery.jwks_uri, fetched.clone());
            fetched
        }
    };

    let jwk = select_key(&jwks, header.kid.as_deref()).ok_or_else(|| {
        AppError::BadRequest("the identity provider did not offer the signing key this token names".to_string())
    })?;

    let key = decoding_key(jwk)?;
    let mut validation = Validation::new(algorithm_for(jwk, &header.alg));
    validation.set_issuer(&[discovery.issuer.as_str()]);
    validation.set_audience(&[provider.client_id.as_str()]);

    let data = decode::<IdTokenClaims>(id_token, &key, &validation)
        .map_err(|error| AppError::Unauthorized(format!("id token rejected: {error}")))?;

    // `jsonwebtoken` checks issuer, audience and expiry. The nonce is ours to
    // check: it is what ties this token to the sign-in we started, and without
    // it an old token for the same user is as good as a fresh one.
    match data.claims.nonce.as_deref() {
        Some(found) if found == nonce => Ok(data.claims),
        _ => Err(AppError::Unauthorized(
            "id token was not issued for this sign-in".to_string(),
        )),
    }
}

fn select_key<'a>(jwks: &'a Jwks, kid: Option<&str>) -> Option<&'a Jwk> {
    match kid {
        // A token naming a key must be checked against that key, not against
        // whichever one happens to work.
        Some(kid) => jwks.keys.iter().find(|key| key.kid.as_deref() == Some(kid)),
        None if jwks.keys.len() == 1 => jwks.keys.first(),
        None => None,
    }
}

fn algorithm_for(jwk: &Jwk, header_alg: &Algorithm) -> Algorithm {
    jwk.alg
        .as_deref()
        .and_then(|alg| serde_json::from_value::<Algorithm>(serde_json::Value::String(alg.into())).ok())
        .unwrap_or(*header_alg)
}

fn decoding_key(jwk: &Jwk) -> Result<DecodingKey, AppError> {
    match jwk.kty.as_str() {
        "RSA" => {
            let (n, e) = (jwk.n.as_deref(), jwk.e.as_deref());
            match (n, e) {
                (Some(n), Some(e)) => DecodingKey::from_rsa_components(n, e)
                    .map_err(|error| AppError::BadRequest(format!("bad RSA signing key: {error}"))),
                _ => Err(AppError::BadRequest("RSA signing key is incomplete".to_string())),
            }
        }
        "EC" => {
            let (x, y) = (jwk.x.as_deref(), jwk.y.as_deref());
            match (x, y) {
                (Some(x), Some(y)) => DecodingKey::from_ec_components(x, y)
                    .map_err(|error| AppError::BadRequest(format!("bad EC signing key: {error}"))),
                _ => Err(AppError::BadRequest("EC signing key is incomplete".to_string())),
            }
        }
        other => Err(AppError::BadRequest(format!(
            "unsupported signing key type: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwk(kid: Option<&str>) -> Jwk {
        Jwk {
            kid: kid.map(str::to_string),
            kty: "RSA".into(),
            alg: Some("RS256".into()),
            n: Some("n".into()),
            e: Some("AQAB".into()),
            crv: None,
            x: None,
            y: None,
        }
    }

    #[test]
    fn the_key_the_token_names_is_the_one_used() {
        let jwks = Jwks { keys: vec![jwk(Some("a")), jwk(Some("b"))] };
        assert_eq!(select_key(&jwks, Some("b")).unwrap().kid.as_deref(), Some("b"));
    }

    #[test]
    fn a_token_naming_an_unknown_key_is_refused() {
        // Not "try the others until one verifies" — that would let a provider
        // key rotation be used to smuggle a token signed by a retired key.
        let jwks = Jwks { keys: vec![jwk(Some("a"))] };
        assert!(select_key(&jwks, Some("missing")).is_none());
    }

    #[test]
    fn a_token_with_no_kid_is_only_accepted_when_there_is_no_ambiguity() {
        assert!(select_key(&Jwks { keys: vec![jwk(None)] }, None).is_some());
        assert!(select_key(&Jwks { keys: vec![jwk(Some("a")), jwk(Some("b"))] }, None).is_none());
    }

    #[test]
    fn an_unsupported_key_type_is_refused_rather_than_ignored() {
        let mut oct = jwk(Some("a"));
        oct.kty = "oct".into();
        assert!(decoding_key(&oct).is_err());
    }

    #[test]
    fn the_providers_declared_algorithm_wins_over_the_tokens_header() {
        // The header is attacker-controlled; the JWKS is not.
        let mut key = jwk(Some("a"));
        key.alg = Some("RS512".into());
        assert_eq!(algorithm_for(&key, &Algorithm::RS256), Algorithm::RS512);
    }

    #[test]
    fn the_header_algorithm_is_used_only_when_the_key_does_not_say() {
        let mut key = jwk(Some("a"));
        key.alg = None;
        assert_eq!(algorithm_for(&key, &Algorithm::ES256), Algorithm::ES256);
    }
}
