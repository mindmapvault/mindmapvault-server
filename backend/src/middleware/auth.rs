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

    pub fn issue_access_token(&self, user_id: &str) -> Result<String, AppError> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.access_expiry_secs);
        let claims = Claims {
            sub: user_id.to_string(),
            typ: ACCESS_TOKEN_TYPE.to_string(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
        };
        Ok(encode(&Header::default(), &claims, &self.encoding_key)?)
    }

    pub fn issue_refresh_token(&self, user_id: &str) -> Result<String, AppError> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.refresh_expiry_secs);
        let claims = Claims {
            sub: user_id.to_string(),
            typ: REFRESH_TOKEN_TYPE.to_string(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
        };
        Ok(encode(&Header::default(), &claims, &self.encoding_key)?)
    }

    pub fn validate_access_token(&self, token: &str) -> Result<Claims, AppError> {
        let data = decode::<Claims>(token, &self.decoding_key, &Validation::default())
            .map_err(|e| AppError::Unauthorized(format!("invalid token: {e}")))?;

        if data.claims.typ != ACCESS_TOKEN_TYPE {
            return Err(AppError::Unauthorized("wrong token type".to_string()));
        }
        Ok(data.claims)
    }

    pub fn validate_refresh_token(&self, token: &str) -> Result<Claims, AppError> {
        let data = decode::<Claims>(token, &self.decoding_key, &Validation::default())
            .map_err(|e| AppError::Unauthorized(format!("invalid token: {e}")))?;

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
