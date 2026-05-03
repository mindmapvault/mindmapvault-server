use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct NewFeedbackSubmission {
    pub public_id: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub subject: String,
    pub message: String,
    pub page_url: Option<String>,
    pub created_at: DateTime<Utc>,
}