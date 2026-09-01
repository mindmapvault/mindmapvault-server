use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct PlanErrorMetadata {
    pub message: String,
    pub code: &'static str,
    pub capability: &'static str,
    pub current_tier: String,
    pub required_tier: Option<String>,
    pub current_value: Option<i64>,
    pub limit_value: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ErrorResponseBody {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    required_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_value: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit_value: Option<i64>,
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),

    /// Throttled. The `u64` is the seconds to wait, sent back as `Retry-After`
    /// so a client knows when to try again instead of hammering.
    #[error("too many requests: {0}")]
    TooManyRequests(String, u64),

    #[error("plan restricted: {0}")]
    PlanRestricted(String, PlanErrorMetadata),

    #[error("database error: {0}")]
    Database(#[from] tokio_postgres::Error),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("jwt error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("internal: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            AppError::NotFound(msg) => {
                tracing::debug!(error_kind = "not_found", "{msg}");
                (
                    StatusCode::NOT_FOUND,
                    ErrorResponseBody {
                        error: msg.clone(),
                        code: None,
                        capability: None,
                        current_tier: None,
                        required_tier: None,
                        current_value: None,
                        limit_value: None,
                    },
                )
            }
            AppError::Unauthorized(msg) => {
                tracing::warn!(error_kind = "unauthorized", "{msg}");
                (
                    StatusCode::UNAUTHORIZED,
                    ErrorResponseBody {
                        error: msg.clone(),
                        code: None,
                        capability: None,
                        current_tier: None,
                        required_tier: None,
                        current_value: None,
                        limit_value: None,
                    },
                )
            }
            AppError::BadRequest(msg) => {
                tracing::debug!(error_kind = "bad_request", "{msg}");
                (
                    StatusCode::BAD_REQUEST,
                    ErrorResponseBody {
                        error: msg.clone(),
                        code: None,
                        capability: None,
                        current_tier: None,
                        required_tier: None,
                        current_value: None,
                        limit_value: None,
                    },
                )
            }
            AppError::Forbidden(msg) => {
                tracing::debug!(error_kind = "forbidden", "{msg}");
                (
                    StatusCode::FORBIDDEN,
                    ErrorResponseBody {
                        error: msg.clone(),
                        code: None,
                        capability: None,
                        current_tier: None,
                        required_tier: None,
                        current_value: None,
                        limit_value: None,
                    },
                )
            }
            AppError::Conflict(msg) => {
                tracing::debug!(error_kind = "conflict", "{msg}");
                (
                    StatusCode::CONFLICT,
                    ErrorResponseBody {
                        error: msg.clone(),
                        code: None,
                        capability: None,
                        current_tier: None,
                        required_tier: None,
                        current_value: None,
                        limit_value: None,
                    },
                )
            }
            AppError::ServiceUnavailable(msg) => {
                tracing::warn!(error_kind = "service_unavailable", "{msg}");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    ErrorResponseBody {
                        error: msg.clone(),
                        code: None,
                        capability: None,
                        current_tier: None,
                        required_tier: None,
                        current_value: None,
                        limit_value: None,
                    },
                )
            }
            AppError::TooManyRequests(msg, retry_after_secs) => {
                tracing::warn!(error_kind = "too_many_requests", retry_after_secs, "{msg}");
                let body = ErrorResponseBody {
                    error: msg.clone(),
                    code: Some("rate_limited".to_string()),
                    capability: None,
                    current_tier: None,
                    required_tier: None,
                    current_value: None,
                    limit_value: None,
                };
                // Returned early: this is the one arm that carries a header, so
                // it cannot go through the shared tuple below.
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(header::RETRY_AFTER, retry_after_secs.to_string())],
                    Json(json!(body)),
                )
                    .into_response();
            }
            AppError::PlanRestricted(msg, metadata) => {
                tracing::debug!(
                    error_kind = "plan_restricted",
                    capability = metadata.capability,
                    current_tier = %metadata.current_tier,
                    "{msg}",
                );
                (
                    StatusCode::FORBIDDEN,
                    ErrorResponseBody {
                        error: msg.clone(),
                        code: Some(metadata.code.to_string()),
                        capability: Some(metadata.capability.to_string()),
                        current_tier: Some(metadata.current_tier.clone()),
                        required_tier: metadata.required_tier.clone(),
                        current_value: metadata.current_value,
                        limit_value: metadata.limit_value,
                    },
                )
            }
            AppError::Database(e) => {
                // Log only the PostgreSQL error code (e.g. "23505"), not the full message —
                // pg DETAIL lines can contain actual column values which may be PII.
                let pg_code = e.code().map(|c| c.code()).unwrap_or("unknown");
                tracing::error!(error_kind = "database", pg_code, "database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponseBody {
                        error: "database error".to_string(),
                        code: None,
                        capability: None,
                        current_tier: None,
                        required_tier: None,
                        current_value: None,
                        limit_value: None,
                    },
                )
            }
            AppError::Storage(msg) => {
                tracing::error!(error_kind = "storage", "{msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponseBody {
                        error: "storage error".to_string(),
                        code: None,
                        capability: None,
                        current_tier: None,
                        required_tier: None,
                        current_value: None,
                        limit_value: None,
                    },
                )
            }
            AppError::Jwt(e) => {
                // Log the error kind (e.g. ExpiredSignature) server-side but return a
                // generic message to the client — JWT library strings can reveal algorithm
                // details that aid token forgery attempts.
                tracing::warn!(error_kind = "jwt", jwt_kind = ?e.kind(), "jwt validation failed");
                (
                    StatusCode::UNAUTHORIZED,
                    ErrorResponseBody {
                        error: "invalid or expired token".to_string(),
                        code: None,
                        capability: None,
                        current_tier: None,
                        required_tier: None,
                        current_value: None,
                        limit_value: None,
                    },
                )
            }
            AppError::Internal(msg) => {
                tracing::error!(error_kind = "internal", "{msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponseBody {
                        error: "internal server error".to_string(),
                        code: None,
                        capability: None,
                        current_tier: None,
                        required_tier: None,
                        current_value: None,
                        limit_value: None,
                    },
                )
            }
        };

        (status, Json(json!(body))).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl AppError {
    pub fn plan_restricted(
        message: impl Into<String>,
        code: &'static str,
        capability: &'static str,
        current_tier: impl Into<String>,
        required_tier: Option<&str>,
        current_value: Option<i64>,
        limit_value: Option<i64>,
    ) -> Self {
        let message = message.into();
        Self::PlanRestricted(
            message.clone(),
            PlanErrorMetadata {
                message,
                code,
                capability,
                current_tier: current_tier.into(),
                required_tier: required_tier.map(ToOwned::to_owned),
                current_value,
                limit_value,
            },
        )
    }
}

