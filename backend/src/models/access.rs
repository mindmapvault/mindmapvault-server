use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionMode {
    PrivateEncrypted,
    SharedPlaintext,
    RealtimeCollaboration,
    Kanban,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UiSurface {
    EncryptedVaultApp,
    SharedMapApp,
    CollaborationApp,
    KanbanApp,
    AdminDashboard,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AccessPlan {
    Free,
    Paid,
}

impl AccessPlan {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Paid => "paid",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AccessSource {
    LegacyBase,
    Stripe,
    AdminOverride,
    DirectGrant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserAccessGrant {
    pub subscription_mode: SubscriptionMode,
    pub ui_surface: UiSurface,
    pub plan: AccessPlan,
    pub source: AccessSource,
    pub granted_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl UserAccessGrant {
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.map(|value| value > now).unwrap_or(true)
    }

    pub fn key(&self) -> (&SubscriptionMode, &UiSurface) {
        (&self.subscription_mode, &self.ui_surface)
    }
}