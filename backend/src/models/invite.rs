//! One-time codes that let a named person sign up while sign-ups are closed.
//!
//! Closing registration without this would lock the operator out of their own
//! instance: an admin cannot create an account directly, because registration
//! is zero-knowledge. The password never reaches the server — the browser
//! derives the account's keys from it and sends only public material — so
//! there is no way for anyone but the new user to make their account. An
//! invite is the server's way of saying "this one person may".
//!
//! The code is stored as written, not hashed, so the console can show it again
//! when the operator loses the tab they copied it from. That is a deliberate
//! trade: an invite is worth one unprivileged account on someone's own server,
//! and anyone who can read this table already holds the admin token or the
//! database itself.

use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};

/// Alphabet without characters that are read wrong when a code is typed off a
/// screen or read down a phone: no O/0, I/1/l, U/V, S/5, Z/2, B/8.
const CODE_ALPHABET: &[u8] = b"ACDEFGHJKMNPQRTWXY34679";

/// Four groups of four. ~72 bits, which is far past anything worth guessing
/// against a rate-limited endpoint, and still short enough to read aloud.
const CODE_GROUPS: usize = 4;
const CODE_GROUP_LEN: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationInvite {
    pub id: String,
    pub code: String,
    /// Free text so the operator remembers who a code was for.
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub used_at: Option<DateTime<Utc>>,
    /// Kept even if that account is later deleted, so the trail survives.
    pub used_by_username: Option<String>,
}

impl RegistrationInvite {
    pub fn is_used(&self) -> bool {
        self.used_at.is_some()
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at <= now)
    }

    /// What the console shows in the status column.
    pub fn status(&self, now: DateTime<Utc>) -> &'static str {
        if self.is_used() {
            "used"
        } else if self.is_expired(now) {
            "expired"
        } else {
            "open"
        }
    }
}

/// Generates a code in the `MMV-XXXX-XXXX-XXXX` shape.
pub fn generate_invite_code() -> String {
    let mut rng = rand::thread_rng();
    let mut groups = Vec::with_capacity(CODE_GROUPS);

    for _ in 0..CODE_GROUPS {
        let group: String = (0..CODE_GROUP_LEN)
            .map(|_| CODE_ALPHABET[rng.gen_range(0..CODE_ALPHABET.len())] as char)
            .collect();
        groups.push(group);
    }

    format!("MMV-{}", groups.join("-"))
}

/// Accepts a code the way a person is likely to have retyped it: any casing,
/// with or without the dashes, with stray spaces.
pub fn normalize_invite_code(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();

    let body = cleaned.strip_prefix("MMV").unwrap_or(&cleaned);
    if body.len() != CODE_GROUPS * CODE_GROUP_LEN {
        // Not the expected shape; hand it back cleaned so the lookup simply
        // misses rather than matching something unintended.
        return cleaned;
    }

    let groups: Vec<String> = body
        .as_bytes()
        .chunks(CODE_GROUP_LEN)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect();

    format!("MMV-{}", groups.join("-"))
}

#[derive(Debug, Deserialize)]
pub struct CreateInviteRequest {
    #[serde(default)]
    pub label: Option<String>,
    /// `None` means the invite does not expire on its own.
    #[serde(default)]
    pub expires_in_days: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_code_has_the_shape_people_will_be_reading_out() {
        let code = generate_invite_code();
        assert!(code.starts_with("MMV-"), "{code}");
        assert_eq!(code.len(), 4 + 4 * 4 + 3, "{code}");
        assert_eq!(code.matches('-').count(), 4, "{code}");
    }

    #[test]
    fn generated_codes_avoid_look_alike_characters() {
        for _ in 0..200 {
            let code = generate_invite_code();
            let body = code.replace('-', "").replace("MMV", "");
            assert!(
                !body.contains(['O', '0', 'I', '1', 'L', 'U', 'V', 'S', '5', 'Z', '2', 'B', '8']),
                "{code}"
            );
        }
    }

    #[test]
    fn codes_are_not_repeated() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..500 {
            assert!(seen.insert(generate_invite_code()), "generated a duplicate");
        }
    }

    #[test]
    fn a_retyped_code_still_matches() {
        let code = generate_invite_code();
        let body = code.replace("MMV-", "").replace('-', "");
        assert_eq!(normalize_invite_code(&code.to_lowercase()), code);
        assert_eq!(normalize_invite_code(&format!("  {code}  ")), code);
        assert_eq!(normalize_invite_code(&body), code);
        assert_eq!(normalize_invite_code(&body.to_lowercase()), code);
    }

    #[test]
    fn nonsense_normalizes_to_something_that_simply_will_not_match() {
        assert_eq!(normalize_invite_code("hello"), "HELLO");
        assert_eq!(normalize_invite_code(""), "");
    }

    #[test]
    fn status_reflects_use_before_expiry() {
        let now = Utc::now();
        let base = RegistrationInvite {
            id: "id".to_string(),
            code: generate_invite_code(),
            label: None,
            created_at: now,
            expires_at: None,
            used_at: None,
            used_by_username: None,
        };

        assert_eq!(base.status(now), "open");

        let expired = RegistrationInvite {
            expires_at: Some(now - chrono::Duration::hours(1)),
            ..base.clone()
        };
        assert_eq!(expired.status(now), "expired");

        // An invite that was used before it ran out reads as used, not expired:
        // what happened to it is more useful than when it would have lapsed.
        let used = RegistrationInvite {
            used_at: Some(now - chrono::Duration::hours(2)),
            ..expired
        };
        assert_eq!(used.status(now), "used");
    }
}
