use axum::{extract::State, routing::post, Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    db::sql_store::DynSqlStore,
    error::AppError,
    models::feedback::NewFeedbackSubmission,
};

#[derive(Clone)]
pub struct PublicState {
    pub db: DynSqlStore,
}

pub fn router(state: PublicState) -> Router {
    Router::new()
        .route("/marketing/feedback", post(submit_feedback))
        .with_state(state)
}

#[derive(Deserialize)]
struct FeedbackRequest {
    name: Option<String>,
    email: Option<String>,
    subject: Option<String>,
    message: String,
    page_url: Option<String>,
}

#[derive(Serialize)]
struct FeedbackResponse {
    message: &'static str,
}

async fn submit_feedback(
    State(state): State<PublicState>,
    Json(body): Json<FeedbackRequest>,
) -> Result<Json<FeedbackResponse>, AppError> {
    let name = body.name.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    let email = body.email.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    let subject = body.subject
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Website feedback".to_string());
    let message = body.message.trim().to_string();
    let page_url = body.page_url.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());

    if let Some(email) = email.as_deref() {
        if !email.contains('@') {
            return Err(AppError::BadRequest("invalid email address".to_string()));
        }
    }

    if let Some(name) = name.as_deref() {
        if name.len() > 120 {
            return Err(AppError::BadRequest("name is too long".to_string()));
        }
    }

    if subject.len() > 160 {
        return Err(AppError::BadRequest("subject is too long".to_string()));
    }

    if message.is_empty() {
        return Err(AppError::BadRequest("message is required".to_string()));
    }

    if message.len() > 4000 {
        return Err(AppError::BadRequest("message is too long".to_string()));
    }

    state
        .db
        .create_feedback_submission(NewFeedbackSubmission {
            public_id: Uuid::new_v4().to_string(),
            name,
            email,
            subject,
            message,
            page_url,
            created_at: Utc::now(),
        })
        .await?;

    Ok(Json(FeedbackResponse {
        message: "feedback saved",
    }))
}