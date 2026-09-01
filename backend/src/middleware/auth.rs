use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

const ACCESS_TOKEN_TYPE: &str = "access";
const REFRESH_TOKEN_TYPE: &str = "refresh";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject — authenticated user id string.
    pub sub: String,
    /// Token type: "access" | "refresh"
    pub typ: String,
    /// Issued-at (Unix timestamp seconds).
    pub iat: i64,
    /// Expiry (Unix timestamp seconds).
    pub exp: i64,
    /// The user's `key_version` when the token was issued. A password
    /// rotation bumps `key_version`, so a mismatch marks a session whose
    /// device still derives keys from the OLD password — its writes would
    /// produce ciphertext nobody can decrypt. `None` on tokens issued before
    /// this claim existed; those die within the access expiry, and `refresh`
    /// refuses them so each device re-authenticates once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kv: Option<u32>,
}

pub struct JwtService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    access_expiry_secs: i64,
    refresh_expiry_secs: i64,
}

impl JwtService {
    pub fn new(secret: &str, access_expiry_secs: u64, refresh_expiry_secs: u64) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret.as_bytes()),
            decoding_key: DecodingKey::from_secret(secret.as_bytes()),
            access_expiry_secs: access_expiry_secs as i64,
            refresh_expiry_secs: refresh_expiry_secs as i64,
        }
    }

    pub fn issue_access_token(&self, user_id: &str, key_version: u32) -> Result<String, AppError> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.access_expiry_secs);
        let claims = Claims {
            sub: user_id.to_string(),
            typ: ACCESS_TOKEN_TYPE.to_string(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            kv: Some(key_version),
        };
        Ok(encode(&Header::default(), &claims, &self.encoding_key)?)
    }

    pub fn issue_refresh_token(&self, user_id: &str, key_version: u32) -> Result<String, AppError> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.refresh_expiry_secs);
        let claims = Claims {
            sub: user_id.to_string(),
            typ: REFRESH_TOKEN_TYPE.to_string(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            kv: Some(key_version),
        };
        Ok(encode(&Header::default(), &claims, &self.encoding_key)?)
    }

    pub fn validate_access_token(&self, token: &str) -> Result<Claims, AppError> {
        let data = decode::<Claims>(token, &self.decoding_key, &Validation::default())
            .map_err(AppError::Jwt)?;

        if data.claims.typ != ACCESS_TOKEN_TYPE {
            return Err(AppError::Unauthorized("wrong token type".to_string()));
        }
        Ok(data.claims)
    }

    pub fn validate_refresh_token(&self, token: &str) -> Result<Claims, AppError> {
        let data = decode::<Claims>(token, &self.decoding_key, &Validation::default())
            .map_err(AppError::Jwt)?;

        if data.claims.typ != REFRESH_TOKEN_TYPE {
            return Err(AppError::Unauthorized("wrong token type".to_string()));
        }
        Ok(data.claims)
    }
}

// ── Axum extractor ────────────────────────────────────────────────────────────

use axum::{
    extract::{FromRef, FromRequestParts},
    http::{request::Parts, HeaderMap},
};
use std::sync::Arc;

/// Newtype carrying the authenticated user's id string.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser(pub String);

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
    Arc<JwtService>: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let jwt = Arc::<JwtService>::from_ref(state);

        let headers: &HeaderMap = &parts.headers;
        let bearer = headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| AppError::Unauthorized("missing Authorization header".to_string()))?;

        let claims = jwt.validate_access_token(bearer)?;
        Ok(AuthenticatedUser(claims.sub))
    }
}

// ── Key-version enforcement for writes ───────────────────────────────────────

use crate::db::sql_store::DynSqlStore;
use std::collections::HashMap;
use std::sync::RwLock;

/// In-process map of user id → current `key_version`, so write requests can
/// be checked against the live value without a database read each time. The
/// same poison-tolerant pattern as `InstanceSettingsHandle`: a poisoned lock
/// only means a writer panicked mid-update, and the map is always internally
/// consistent, so reads proceed on the inner value either way.
#[derive(Clone, Default)]
pub struct KeyVersionCache(Arc<RwLock<HashMap<String, u32>>>);

impl KeyVersionCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, user_id: &str) -> Option<u32> {
        let map = self.0.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        map.get(user_id).copied()
    }

    pub fn set(&self, user_id: &str, key_version: u32) {
        let mut map = self.0.write().unwrap_or_else(|poisoned| poisoned.into_inner());
        map.insert(user_id.to_string(), key_version);
    }
}

/// Authenticated user for endpoints that WRITE user data.
///
/// Beyond the checks in [`AuthenticatedUser`], this rejects tokens whose `kv`
/// claim no longer matches the user's current `key_version`. After a password
/// rotation every other signed-in device still holds keys derived from the
/// old password; a write from such a session stores titles or attachment key
/// wraps that no password can ever decrypt again. Failing the write forces
/// that device back through sign-in, where only the new password works.
///
/// Read endpoints stay on [`AuthenticatedUser`]: a stale reader sees titles
/// it cannot decrypt, which is confusing but destroys nothing.
///
/// Tokens without a `kv` claim (issued before the claim existed) are let
/// through — they expire within the access-token lifetime, and `refresh`
/// refuses to renew them.
#[derive(Debug, Clone)]
pub struct VerifiedWriter(pub String);

impl<S> FromRequestParts<S> for VerifiedWriter
where
    S: Send + Sync,
    Arc<JwtService>: FromRef<S>,
    DynSqlStore: FromRef<S>,
    KeyVersionCache: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let jwt = Arc::<JwtService>::from_ref(state);

        let headers: &HeaderMap = &parts.headers;
        let bearer = headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| AppError::Unauthorized("missing Authorization header".to_string()))?;

        let claims = jwt.validate_access_token(bearer)?;

        if let Some(token_kv) = claims.kv {
            let cache = KeyVersionCache::from_ref(state);
            let current = match cache.get(&claims.sub) {
                Some(kv) => kv,
                None => {
                    // Cache cold (fresh process). One read seeds it.
                    let db = DynSqlStore::from_ref(state);
                    let kv = db
                        .load_user_key_version(&claims.sub)
                        .await?
                        .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?;
                    cache.set(&claims.sub, kv);
                    kv
                }
            };
            if token_kv != current {
                return Err(AppError::Unauthorized(
                    "this session was signed in before the password changed — sign in again with the new password".to_string(),
                ));
            }
        }

        Ok(VerifiedWriter(claims.sub))
    }
}
