//! Instance-wide settings an operator can change without restarting the server.
//!
//! A self-hosted instance has no billing tier to lean on, so the controls that
//! keep a public deployment from being scripted into unlimited storage — or its
//! login form from being brute-forced — have to live somewhere the operator can
//! reach. They live in one database row, edited from the admin console.
//!
//! Environment variables seed the row the first time the server starts against
//! an empty database and are ignored afterwards. That keeps a single source of
//! truth: once the row exists, the admin console is the authority, and an
//! operator is never left wondering why the env var they just changed had no
//! effect on a running deployment.

use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Value meaning "no limit" for the byte caps, so an upgrade of an existing
/// instance keeps behaving the way it did before this setting existed.
pub const UNLIMITED: i64 = 0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceSettings {
    /// When false, `POST /api/auth/register` is refused and the app hides the
    /// sign-up form. Existing accounts are unaffected.
    pub registration_enabled: bool,
    /// Per-account storage cap in bytes; `UNLIMITED` disables it.
    pub user_storage_limit_bytes: i64,
    /// Largest single attachment in bytes; `UNLIMITED` disables it. The
    /// transport still caps a request body at `MAX_UPLOAD_BODY_BYTES`.
    pub max_attachment_size_bytes: i64,
    /// Auth requests allowed per client address per minute; 0 disables it.
    pub auth_rate_limit_per_minute: i32,
    /// Consecutive failed logins before an account is throttled; 0 disables it.
    pub failed_login_threshold: i32,
    /// How long that throttle lasts.
    pub failed_login_lockout_minutes: i32,
    /// Whether `X-Forwarded-For` may identify the client. Only turn this on
    /// when a proxy you control sets the header — otherwise a caller can spoof
    /// it and step around both throttles.
    pub trust_proxy_headers: bool,
    pub updated_at: DateTime<Utc>,
}

impl Default for InstanceSettings {
    fn default() -> Self {
        Self {
            registration_enabled: true,
            user_storage_limit_bytes: UNLIMITED,
            max_attachment_size_bytes: UNLIMITED,
            auth_rate_limit_per_minute: 30,
            failed_login_threshold: 10,
            failed_login_lockout_minutes: 15,
            trust_proxy_headers: false,
            updated_at: Utc::now(),
        }
    }
}

impl InstanceSettings {
    /// The seed values for a database that has no settings row yet.
    ///
    /// Only the variables an operator is likely to want set before the first
    /// boot are read; everything else starts at its default and is changed from
    /// the admin console.
    pub fn from_env_seed() -> Self {
        let mut settings = Self::default();

        if let Some(value) = env_bool("REGISTRATION_ENABLED") {
            settings.registration_enabled = value;
        }
        if let Some(value) = env_i64("USER_STORAGE_LIMIT_BYTES") {
            settings.user_storage_limit_bytes = value.max(0);
        }
        if let Some(value) = env_i64("MAX_ATTACHMENT_SIZE_BYTES") {
            settings.max_attachment_size_bytes = value.max(0);
        }
        if let Some(value) = env_bool("TRUST_PROXY_HEADERS") {
            settings.trust_proxy_headers = value;
        }

        settings
    }

    pub fn storage_limit(&self) -> Option<i64> {
        (self.user_storage_limit_bytes > UNLIMITED).then_some(self.user_storage_limit_bytes)
    }

    pub fn attachment_limit(&self) -> Option<i64> {
        (self.max_attachment_size_bytes > UNLIMITED).then_some(self.max_attachment_size_bytes)
    }
}

/// The fields the admin console may change. All optional: a request carries
/// only what the operator edited.
#[derive(Debug, Default, Deserialize)]
pub struct UpdateInstanceSettingsRequest {
    pub registration_enabled: Option<bool>,
    pub user_storage_limit_bytes: Option<i64>,
    pub max_attachment_size_bytes: Option<i64>,
    pub auth_rate_limit_per_minute: Option<i32>,
    pub failed_login_threshold: Option<i32>,
    pub failed_login_lockout_minutes: Option<i32>,
    pub trust_proxy_headers: Option<bool>,
}

impl UpdateInstanceSettingsRequest {
    /// Applies the request to `current`, rejecting values that would make the
    /// instance unusable rather than storing them and failing later.
    pub fn apply(self, current: &InstanceSettings) -> Result<InstanceSettings, String> {
        let mut next = current.clone();

        if let Some(value) = self.registration_enabled {
            next.registration_enabled = value;
        }
        if let Some(value) = self.user_storage_limit_bytes {
            if value < 0 {
                return Err("user_storage_limit_bytes must be 0 (unlimited) or more".to_string());
            }
            next.user_storage_limit_bytes = value;
        }
        if let Some(value) = self.max_attachment_size_bytes {
            if value < 0 {
                return Err("max_attachment_size_bytes must be 0 (unlimited) or more".to_string());
            }
            next.max_attachment_size_bytes = value;
        }
        if let Some(value) = self.auth_rate_limit_per_minute {
            if !(0..=100_000).contains(&value) {
                return Err("auth_rate_limit_per_minute must be between 0 and 100000".to_string());
            }
            next.auth_rate_limit_per_minute = value;
        }
        if let Some(value) = self.failed_login_threshold {
            if !(0..=1_000).contains(&value) {
                return Err("failed_login_threshold must be between 0 and 1000".to_string());
            }
            next.failed_login_threshold = value;
        }
        if let Some(value) = self.failed_login_lockout_minutes {
            // An unbounded lockout would let one attacker park an account
            // offline indefinitely, so cap it at a day.
            if !(1..=1_440).contains(&value) {
                return Err("failed_login_lockout_minutes must be between 1 and 1440".to_string());
            }
            next.failed_login_lockout_minutes = value;
        }
        if let Some(value) = self.trust_proxy_headers {
            next.trust_proxy_headers = value;
        }

        next.updated_at = Utc::now();
        Ok(next)
    }
}

/// Shared, in-process copy of the settings row.
///
/// Every request that enforces a limit reads this instead of the database. It
/// is refreshed when the admin console writes, and on a timer so a deployment
/// running more than one replica converges without a restart.
#[derive(Clone)]
pub struct InstanceSettingsHandle(Arc<RwLock<InstanceSettings>>);

impl InstanceSettingsHandle {
    pub fn new(settings: InstanceSettings) -> Self {
        Self(Arc::new(RwLock::new(settings)))
    }

    pub fn get(&self) -> InstanceSettings {
        // A poisoned lock means a writer panicked mid-update. The settings are
        // plain values with no invariant spanning them, so the last-written
        // state is still safe to read — far better than taking the process
        // down over a config cache.
        match self.0.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub fn set(&self, settings: InstanceSettings) {
        match self.0.write() {
            Ok(mut guard) => *guard = settings,
            Err(poisoned) => *poisoned.into_inner() = settings,
        }
    }
}

fn env_bool(key: &str) -> Option<bool> {
    let raw = std::env::var(key).ok()?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        "" => None,
        other => {
            tracing::warn!("ignoring {key}: '{other}' is not a boolean");
            None
        }
    }
}

fn env_i64(key: &str) -> Option<i64> {
    let raw = std::env::var(key).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<i64>() {
        Ok(value) => Some(value),
        Err(_) => {
            tracing::warn!("ignoring {key}: '{trimmed}' is not a whole number");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_is_the_default_so_upgrades_do_not_start_rejecting_uploads() {
        let settings = InstanceSettings::default();
        assert_eq!(settings.storage_limit(), None);
        assert_eq!(settings.attachment_limit(), None);
        assert!(settings.registration_enabled);
    }

    #[test]
    fn update_leaves_untouched_fields_alone() {
        let current = InstanceSettings::default();
        let next = UpdateInstanceSettingsRequest {
            registration_enabled: Some(false),
            ..Default::default()
        }
        .apply(&current)
        .expect("valid update");

        assert!(!next.registration_enabled);
        assert_eq!(next.auth_rate_limit_per_minute, current.auth_rate_limit_per_minute);
    }

    #[test]
    fn negative_limits_are_refused() {
        let current = InstanceSettings::default();
        assert!(UpdateInstanceSettingsRequest {
            user_storage_limit_bytes: Some(-1),
            ..Default::default()
        }
        .apply(&current)
        .is_err());
    }

    #[test]
    fn a_lockout_cannot_be_set_to_never_expire() {
        let current = InstanceSettings::default();
        assert!(UpdateInstanceSettingsRequest {
            failed_login_lockout_minutes: Some(0),
            ..Default::default()
        }
        .apply(&current)
        .is_err());
        assert!(UpdateInstanceSettingsRequest {
            failed_login_lockout_minutes: Some(10_000),
            ..Default::default()
        }
        .apply(&current)
        .is_err());
    }
}
