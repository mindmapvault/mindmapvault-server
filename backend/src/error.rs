use axum::{
    http::StatusCode,
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

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),

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
            AppError::NotFound(msg) => (
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
            ),
            AppError::Unauthorized(msg) => (
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
            ),
            AppError::BadRequest(msg) => (
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
            ),
            AppError::Conflict(msg) => (
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
            ),
            AppError::ServiceUnavailable(msg) => (
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
            ),
            AppError::PlanRestricted(msg, metadata) => (
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
            ),
            AppError::Database(e) => {
                tracing::error!("database error: {e}");
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
                tracing::error!("storage error: {msg}");
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
            AppError::Jwt(e) => (
                StatusCode::UNAUTHORIZED,
                ErrorResponseBody {
                    error: e.to_string(),
                    code: None,
                    capability: None,
                    current_tier: None,
                    required_tier: None,
                    current_value: None,
                    limit_value: None,
                },
            ),
            AppError::Internal(msg) => {
                tracing::error!("internal error: {msg}");
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

