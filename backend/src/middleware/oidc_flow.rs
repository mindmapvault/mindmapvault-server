//! The short-lived state of an OpenID Connect sign-in that is in flight.
//!
//! Between the redirect out to the provider and the callback coming back, three
//! things have to survive and be checked on return:
//!
//! * **state** — proves the callback belongs to a sign-in this server started,
//!   and not to a link an attacker sent the user (CSRF on the login flow);
//! * **nonce** — proves the ID token was minted for *this* request and is not a
//!   replay of an older one;
//! * **PKCE verifier** — proves whoever redeems the authorization code is who
//!   asked for it, so an intercepted code is worthless on its own.
//!
//! Kept in memory rather than in a cookie or the database. A cookie would have
//! to be signed and would ride on every request; the database would mean a
//! write per sign-in attempt, which is traffic an attacker controls. The cost
//! is that a sign-in started on one replica must come back to the same one —
//! see the note in docs/DEPLOYMENT.md.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine as _};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// How long a user has to get through the provider's screens.
///
/// Long enough for a password manager, a second factor and a consent page;
/// short enough that abandoned attempts do not accumulate.
const FLOW_TTL: Duration = Duration::from_secs(10 * 60);

/// What was remembered when the redirect went out.
#[derive(Debug, Clone)]
pub struct PendingFlow {
    pub provider_id: String,
    pub nonce: String,
    pub pkce_verifier: String,
    /// Where to send the browser once the sign-in completes. Always a path on
    /// this server — never taken from the request, or the callback becomes an
    /// open redirect.
    pub return_path: String,
    started_at: Instant,
}

#[derive(Default)]
pub struct OidcFlows {
    pending: Mutex<HashMap<String, PendingFlow>>,
}

impl OidcFlows {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remembers a sign-in and returns its `state` parameter.
    pub fn begin(
        &self,
        provider_id: &str,
        nonce: String,
        pkce_verifier: String,
        return_path: String,
    ) -> String {
        let state = random_token();
        let mut pending = self.lock();
        pending.insert(
            state.clone(),
            PendingFlow {
                provider_id: provider_id.to_string(),
                nonce,
                pkce_verifier,
                return_path,
                started_at: Instant::now(),
            },
        );
        state
    }

    /// Takes the flow for `state`, if it exists and has not expired.
    ///
    /// Removing it is the point: a `state` is good for exactly one callback, so
    /// replaying the same code and state cannot mint a second session.
    pub fn take(&self, state: &str) -> Option<PendingFlow> {
        let mut pending = self.lock();
        let flow = pending.remove(state)?;
        if flow.started_at.elapsed() > FLOW_TTL {
            return None;
        }
        Some(flow)
    }

    /// Drops abandoned sign-ins. Called from the same timer as the throttle.
    pub fn prune(&self) {
        let mut pending = self.lock();
        pending.retain(|_, flow| flow.started_at.elapsed() <= FLOW_TTL);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, PendingFlow>> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// 32 bytes of randomness, URL-safe. Used for `state`, `nonce` and the PKCE
/// verifier, all of which must be unguessable.
pub fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    B64URL.encode(bytes)
}

/// The S256 challenge for a PKCE verifier.
pub fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    B64URL.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flows() -> OidcFlows {
        OidcFlows::new()
    }

    #[test]
    fn a_flow_comes_back_once_and_only_once() {
        let flows = flows();
        let state = flows.begin("p1", "n".into(), "v".into(), "/vaults".into());

        let first = flows.take(&state).expect("first callback is honoured");
        assert_eq!(first.provider_id, "p1");
        // A replayed callback must not mint a second session.
        assert!(flows.take(&state).is_none());
    }

    #[test]
    fn an_unknown_state_is_refused() {
        assert!(flows().take("never-issued").is_none());
    }

    #[test]
    fn each_sign_in_gets_its_own_state() {
        let flows = flows();
        let a = flows.begin("p1", "n".into(), "v".into(), "/".into());
        let b = flows.begin("p1", "n".into(), "v".into(), "/".into());
        assert_ne!(a, b);
    }

    #[test]
    fn tokens_are_long_enough_to_be_unguessable() {
        // 32 bytes, base64url, no padding.
        assert_eq!(random_token().len(), 43);
        assert_ne!(random_token(), random_token());
    }

    #[test]
    fn the_pkce_challenge_matches_the_rfc_7636_example() {
        // RFC 7636 appendix B, so a provider's own checker agrees with ours.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
