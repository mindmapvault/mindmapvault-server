use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::access::{AccessPlan, AccessSource, SubscriptionMode, UiSurface, UserAccessGrant};

pub const CLOUD_FREE_LIMIT_BYTES: i64 = 25 * 1024 * 1024;
pub const CLOUD_PAID_LIMIT_BYTES: i64 = 250 * 1024 * 1024;
pub const CLOUD_FREE_MAX_ATTACHMENT_SIZE_BYTES: i64 = 5 * 1024 * 1024;
pub const CLOUD_PAID_MAX_ATTACHMENT_SIZE_BYTES: i64 = 50 * 1024 * 1024;
pub const CLOUD_FREE_MAX_ACTIVE_SHARES: i64 = 3;
pub const CLOUD_PAID_MAX_ACTIVE_SHARES: i64 = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionTier {
    Free,
    Paid,
}

impl Default for SubscriptionTier {
    fn default() -> Self {
        Self::Free
    }
}

impl SubscriptionTier {
    pub fn storage_limit_bytes(&self) -> i64 {
        match self {
            Self::Free => CLOUD_FREE_LIMIT_BYTES,
            Self::Paid => CLOUD_PAID_LIMIT_BYTES,
        }
    }

    pub fn max_attachment_size_bytes(&self) -> i64 {
        match self {
            Self::Free => CLOUD_FREE_MAX_ATTACHMENT_SIZE_BYTES,
            Self::Paid => CLOUD_PAID_MAX_ATTACHMENT_SIZE_BYTES,
        }
    }

    pub fn max_active_shares(&self) -> i64 {
        match self {
            Self::Free => CLOUD_FREE_MAX_ACTIVE_SHARES,
            Self::Paid => CLOUD_PAID_MAX_ACTIVE_SHARES,
        }
    }

    pub fn can_create_public_shares(&self) -> bool {
        true
    }

    pub fn can_include_attachments_in_shares(&self) -> bool {
        matches!(self, Self::Paid)
    }

    pub fn can_use_plaintext_collaboration(&self) -> bool {
        true
    }

    pub fn can_export_large_maps(&self) -> bool {
        matches!(self, Self::Paid)
    }

    pub fn can_use_admin_controls(&self) -> bool {
        false
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Paid => "paid",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "paid" => Self::Paid,
            _ => Self::Free,
        }
    }
}

/// Argon2id parameters stored alongside the salt so we can re-derive
/// the master key on the client without hardcoding parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Argon2Params {
    pub m_cost: u32, // memory in KiB   (default 65536 = 64 MiB)
    pub t_cost: u32, // iterations       (default 3)
    pub p_cost: u32, // parallelism      (default 4)
}

impl Default for Argon2Params {
    fn default() -> Self {
        Self {
            m_cost: 65_536,
            t_cost: 3,
            p_cost: 4,
        }
    }
}

pub fn effective_access_grants_from_legacy(
    stored_grants: &[UserAccessGrant],
    effective_tier: SubscriptionTier,
    effective_plan_source: &str,
    created_at: DateTime<Utc>,
    subscription_current_period_end: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Vec<UserAccessGrant> {
    let mut grants: Vec<UserAccessGrant> = stored_grants
        .iter()
        .filter(|grant| grant.is_active(now))
        .cloned()
        .collect();

    let has_private_encrypted = grants.iter().any(|grant| {
        matches!(grant.subscription_mode, SubscriptionMode::PrivateEncrypted)
            && matches!(grant.ui_surface, UiSurface::EncryptedVaultApp)
    });

    if !has_private_encrypted {
        grants.push(UserAccessGrant {
            subscription_mode: SubscriptionMode::PrivateEncrypted,
            ui_surface: UiSurface::EncryptedVaultApp,
            plan: match effective_tier {
                SubscriptionTier::Free => AccessPlan::Free,
                SubscriptionTier::Paid => AccessPlan::Paid,
            },
            source: match effective_plan_source {
                "admin_override" => AccessSource::AdminOverride,
                "stripe" => AccessSource::Stripe,
                _ => AccessSource::LegacyBase,
            },
            granted_at: created_at,
            expires_at: subscription_current_period_end,
            note: Some("Legacy encrypted app access".to_string()),
        });
    }

    grants
}

// ── DTOs ──────────────────────────────────────────────────────────────────────

/// The client's public key bundle sent at registration.
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    /// HKDF(master_key, "crypt-mind-auth-v1")
    pub auth_token: String,
    /// Random 16-byte salt (base64) used in Argon2id.
    pub argon2_salt: String,
    pub argon2_params: Argon2Params,
    pub classical_public_key: String,
    pub pq_public_key: String,
    pub classical_priv_encrypted: String,
    pub pq_priv_encrypted: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    /// HKDF(master_key, "crypt-mind-auth-v1")
    pub auth_token: String,
}

/// Returned on successful login — includes the encrypted key bundle so the
/// client can decrypt its private keys.
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub classical_public_key: String,
    pub pq_public_key: String,
    pub classical_priv_encrypted: String,
    pub pq_priv_encrypted: String,
    pub argon2_salt: String,
    pub argon2_params: Argon2Params,
    pub key_version: u32,
}

/// Returned by GET /api/auth/salt — all the client needs to re-derive master_key.
#[derive(Debug, Serialize)]
pub struct SaltResponse {
    pub argon2_salt: String,
    pub argon2_params: Argon2Params,
}

/// Returned by GET /api/auth/keys (authenticated).
/// Includes the argon2 parameters so the client can re-derive the master key
/// for credential rotation without a separate unauthenticated salt request.
#[derive(Debug, Serialize)]
pub struct KeyBundleResponse {
    pub classical_public_key: String,
    pub pq_public_key: String,
    pub classical_priv_encrypted: String,
    pub pq_priv_encrypted: String,
    pub argon2_salt: String,
    pub argon2_params: Argon2Params,
    pub key_version: u32,
}

/// Request body for POST /api/auth/rotate-credentials.
#[derive(Debug, Deserialize)]
pub struct RotateCredentialsRequest {
    /// HKDF(old_master_key, "crypt-mind-auth-v1") — proves the caller knows the current password.
    pub current_auth_token: String,
    /// HKDF(new_master_key, "crypt-mind-auth-v1") — the server hashes this before storing.
    pub new_auth_token: String,
    pub new_argon2_salt: String,
    pub new_argon2_params: Argon2Params,
    pub new_classical_priv_encrypted: String,
    pub new_pq_priv_encrypted: String,
    pub new_key_version: u32,
    /// Every vault owned by the user re-encrypted under the new title key.
    /// The server rejects partial bundles — missing vaults would become
    /// unreadable after rotation.
    pub updated_vaults: Vec<RotateVaultApiEntry>,
}

#[derive(Debug, Deserialize)]
pub struct RotateVaultApiEntry {
    pub id: String,
    pub title_encrypted: String,
    /// None → keep existing note; Some("") → clear note; Some(ct) → update.
    pub vault_note_encrypted: Option<String>,
}

/// Optional profile update sent by the client.
#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
}

/// Profile returned to the authenticated client.
#[derive(Debug, Serialize)]
pub struct ProfileResponse {
    pub username: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: Option<String>,
    pub subscription_tier: String,
    pub storage_limit_bytes: i64,
    pub subscription_current_period_end: Option<DateTime<Utc>>,
    pub access_grants: Vec<UserAccessGrant>,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionSummaryResponse {
    pub subscription_tier: String,
    pub plan_source: String,
    pub storage_limit_bytes: i64,
    pub stripe_customer_id_present: bool,
    pub stripe_subscription_status: Option<String>,
    pub subscription_current_period_end: Option<DateTime<Utc>>,
    pub manual_override_active: bool,
}

#[derive(Debug, Serialize)]
pub struct AccountStorageResponse {
    pub total_bytes: i64,
    pub attachment_count: usize,
    pub attachment_bytes: i64,
    pub plan_tier: String,
    pub plan_limit_bytes: i64,
    pub remaining_bytes: i64,
    pub over_limit: bool,
    pub vault_count: usize,
}

#[derive(Debug, Serialize)]
pub struct AccountCapabilitiesResponse {
    pub plan_tier: String,
    pub storage_limit_bytes: i64,
    pub max_attachment_size_bytes: i64,
    pub max_active_shares: i64,
    pub can_create_public_shares: bool,
    pub can_include_attachments_in_shares: bool,
    pub can_use_plaintext_collaboration: bool,
    pub can_export_large_maps: bool,
    pub can_use_admin_controls: bool,
}
