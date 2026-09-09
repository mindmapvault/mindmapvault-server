//! Alternative ways to unlock an account.
//!
//! Everything a user owns hangs off one 32-byte master key. Today that key is
//! derived from their passphrase with Argon2, which means the passphrase has to
//! be typed on every device, every time the session keys are gone.
//!
//! Nothing downstream cares *how* those bytes were obtained. So an account can
//! keep additional copies of the same master key, each encrypted under a
//! different secret, and unlock by decrypting one instead of deriving it. The
//! passphrase path is untouched — this is purely additive, and no vault is ever
//! re-encrypted.
//!
//! The server holds only ciphertext. The key that opens it never leaves the
//! browser, which is the line between this and key escrow.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// What is holding the wrapping key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnlockKind {
    /// A non-extractable key kept in this browser's storage. Convenience on a
    /// machine the user has said they trust; anyone with the browser profile
    /// can use it, which is why it is opt-in and revocable.
    Device,
    /// A secret derived from a passkey via the WebAuthn PRF extension. Not
    /// implemented yet; named here so the stored `kind` does not have to change
    /// when it is.
    WebauthnPrf,
}

impl UnlockKind {
    pub fn as_str(self) -> &'static str {
        match self {
            UnlockKind::Device => "device",
            UnlockKind::WebauthnPrf => "webauthn-prf",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "device" => Some(UnlockKind::Device),
            "webauthn-prf" => Some(UnlockKind::WebauthnPrf),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnlockMethod {
    pub id: String,
    pub user_id: String,
    pub kind: UnlockKind,
    /// Shown when listing trusted devices, so someone can tell which is which.
    pub label: String,
    /// The master key, encrypted under this method's key. Opaque here.
    pub wrapped_master_key: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// What the client sends to trust a device. The wrapping key stays in the
/// browser; only what it produced is sent.
#[derive(Debug, Deserialize)]
pub struct RegisterUnlockMethodRequest {
    /// Chosen by the client so it can find its own row again without having to
    /// store a server-assigned id separately from the key it belongs to.
    pub id: String,
    pub kind: UnlockKind,
    #[serde(default)]
    pub label: String,
    pub wrapped_master_key: String,
}

/// What the account settings screen lists. Never the wrapped key: that is
/// fetched one at a time, by the device that can actually open it.
#[derive(Debug, Serialize)]
pub struct UnlockMethodSummary {
    pub id: String,
    pub kind: UnlockKind,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

impl From<&UnlockMethod> for UnlockMethodSummary {
    fn from(method: &UnlockMethod) -> Self {
        Self {
            id: method.id.clone(),
            kind: method.kind,
            label: method.label.clone(),
            created_at: method.created_at,
            last_used_at: method.last_used_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WrappedMasterKeyResponse {
    pub wrapped_master_key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip_through_their_stored_spelling() {
        for kind in [UnlockKind::Device, UnlockKind::WebauthnPrf] {
            assert_eq!(UnlockKind::parse(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn an_unknown_kind_is_refused_rather_than_guessed() {
        assert_eq!(UnlockKind::parse("escrow"), None);
        assert_eq!(UnlockKind::parse(""), None);
    }

    #[test]
    fn the_wire_spelling_is_kebab_case() {
        // The stored string and the JSON have to agree, or a row written by one
        // path is unreadable by the other.
        let json = serde_json::to_string(&UnlockKind::WebauthnPrf).expect("serialises");
        assert_eq!(json, "\"webauthn-prf\"");
        assert_eq!(json.trim_matches('"'), UnlockKind::WebauthnPrf.as_str());
    }

    #[test]
    fn a_summary_never_carries_the_wrapped_key() {
        let method = UnlockMethod {
            id: "d1".into(),
            user_id: "u1".into(),
            kind: UnlockKind::Device,
            label: "Work laptop".into(),
            wrapped_master_key: "SECRETCIPHERTEXT".into(),
            created_at: Utc::now(),
            last_used_at: None,
        };
        let json = serde_json::to_string(&UnlockMethodSummary::from(&method)).expect("serialises");
        assert!(!json.contains("SECRETCIPHERTEXT"), "wrapped key leaked into: {json}");
    }
}
