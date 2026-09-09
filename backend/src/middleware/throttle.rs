//! Per-address request throttling and failed-login lockout for the auth routes.
//!
//! Kept in process memory on purpose. The state is small, worthless to persist
//! (a restart clearing it is an acceptable trade), and putting it in Postgres
//! would mean a write on every failed password attempt — exactly the traffic an
//! attacker controls.
//!
//! Two separate mechanisms, because they answer different attacks:
//!
//! * the **address bucket** limits how fast one caller can hit the auth routes
//!   at all, which is what stops a credential-stuffing run;
//! * the **failed-login counter** parks one credential after repeated wrong
//!   passwords, which is what stops a slow guess against a known username.
//!
//! The second one can be turned against a user: anyone who knows a username can
//! keep it locked by failing logins on purpose. That is why the lockout is
//! measured in minutes rather than being permanent, why it is a separate knob
//! an operator can switch off, and why it is scoped to the **credential** and
//! not the account — see `CredentialKind`. The admin `is_locked` flag is
//! untouched by any of this — that one is a deliberate act by a person.
//!
//! Neither of these bounds the work a *distributed* attacker can cause, since
//! both are keyed on something the attacker can spread across. That is what
//! `middleware::verify_budget` is for.

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

/// The period the configured limits are expressed over: "requests per minute".
const WINDOW: Duration = Duration::from_secs(60);

/// What a request costs the server, and therefore which allowance it spends.
///
/// Named for the kind of work rather than for a route, so a federated sign-in
/// callback has somewhere to attach without another setting being invented for
/// it: it is a `Credential` attempt like any other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttemptClass {
    /// An indexed read and nothing more — the salt lookup. Cheap enough that
    /// throttling it hard only inconveniences real clients, which is why it no
    /// longer shares an allowance with the routes below.
    Lookup,
    /// Anything that verifies or establishes a credential: sign-in, sign-up,
    /// and in time the federated callback. These run Argon2 or talk to an
    /// identity provider, so they are orders of magnitude more expensive.
    Credential,
}

/// Which credential a failure was against.
///
/// Lockout is scoped to this rather than to the account. An account may hold
/// more than one credential — a password *and* a federated identity — and
/// locking the account on failed passwords would let an attacker who cannot
/// guess the password deny the user their federated sign-in instead. That turns
/// a brute-force defence into a denial-of-service tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CredentialKind {
    Password,
}

/// A token bucket: `limit` tokens a minute, and up to a minute's worth saved up.
///
/// This replaced a fixed window, which reset on a boundary and so allowed twice
/// the limit in the couple of seconds either side of one and then nothing for
/// the rest of the minute. A bucket spends the same tokens per minute without
/// that cliff, and lets a genuine flurry — a page reload storm, say — through
/// on saved-up allowance.
#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

#[derive(Debug, Clone, Copy)]
struct Failures {
    count: i32,
    last_seen: Instant,
    locked_until: Option<Instant>,
}

#[derive(Default)]
struct ThrottleState {
    addresses: HashMap<(IpAddr, AttemptClass), Bucket>,
    failures: HashMap<(String, CredentialKind), Failures>,
}

#[derive(Default)]
pub struct AuthThrottle {
    state: Mutex<ThrottleState>,
}

impl AuthThrottle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spends one token from `address`'s allowance for `class`. Returns how
    /// long the caller has to wait when the bucket is empty.
    ///
    /// `limit` of 0 disables the check, which is how an operator opts out.
    /// Each class has its own bucket, so a burst of cheap lookups cannot use up
    /// the allowance that protects the expensive routes.
    pub fn check_address(
        &self,
        address: IpAddr,
        class: AttemptClass,
        limit: i32,
    ) -> Result<(), Duration> {
        if limit <= 0 {
            return Ok(());
        }

        let capacity = limit as f64;
        let per_second = capacity / WINDOW.as_secs_f64();
        let now = Instant::now();
        let mut state = self.lock();
        let bucket = state
            .addresses
            .entry((address, class))
            .or_insert(Bucket {
                tokens: capacity,
                last_refill: now,
            });

        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * per_second).min(capacity);
        bucket.last_refill = now;

        if bucket.tokens < 1.0 {
            let shortfall = 1.0 - bucket.tokens;
            return Err(Duration::from_secs_f64(shortfall / per_second));
        }

        bucket.tokens -= 1.0;
        Ok(())
    }

    /// How long this credential stays locked out after too many wrong attempts.
    pub fn lockout_remaining(&self, username: &str, kind: CredentialKind) -> Option<Duration> {
        let now = Instant::now();
        let mut state = self.lock();
        let key = (normalize(username), kind);
        let record = state.failures.get(&key)?;
        let locked_until = record.locked_until?;

        if locked_until <= now {
            // Expired: clear it so a returning user starts from a clean slate
            // rather than one failure away from being locked again.
            state.failures.remove(&key);
            return None;
        }

        Some(locked_until.duration_since(now))
    }

    /// Records a wrong credential. Locks it once `threshold` is reached.
    pub fn record_failure(
        &self,
        username: &str,
        kind: CredentialKind,
        threshold: i32,
        lockout_minutes: i32,
    ) {
        if threshold <= 0 {
            return;
        }

        let now = Instant::now();
        let lockout = Duration::from_secs(lockout_minutes.max(1) as u64 * 60);
        let mut state = self.lock();
        let record = state
            .failures
            .entry((normalize(username), kind))
            .or_insert(Failures {
                count: 0,
                last_seen: now,
                locked_until: None,
            });

        // Failures spaced further apart than one lockout window are not the
        // same attack; start counting again rather than accumulating a lockout
        // over weeks of occasional typos.
        if now.duration_since(record.last_seen) > lockout {
            record.count = 0;
        }

        record.count += 1;
        record.last_seen = now;
        if record.count >= threshold {
            record.locked_until = Some(now + lockout);
            record.count = 0;
        }
    }

    /// Clears the counter after a successful sign-in with this credential.
    pub fn record_success(&self, username: &str, kind: CredentialKind) {
        self.lock().failures.remove(&(normalize(username), kind));
    }

    /// Drops entries nothing is waiting on. Called from a background timer so
    /// the maps do not grow with every address that ever touched the server.
    pub fn prune(&self) {
        let now = Instant::now();
        let mut state = self.lock();
        // A bucket untouched for a full window has refilled to capacity, so
        // forgetting it is the same as keeping it.
        state
            .addresses
            .retain(|_, bucket| now.duration_since(bucket.last_refill) < WINDOW);
        state.failures.retain(|_, record| {
            record.locked_until.is_some_and(|until| until > now)
                || now.duration_since(record.last_seen) < Duration::from_secs(24 * 60 * 60)
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ThrottleState> {
        // Nothing here is left half-updated by a panic, so a poisoned lock is
        // recoverable and preferable to failing every subsequent login.
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Usernames are matched case-insensitively so `Alice` and `alice` cannot be
/// used to double an attacker's allowance against one account.
fn normalize(username: &str) -> String {
    username.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address() -> IpAddr {
        "203.0.113.7".parse().expect("valid address")
    }

    #[test]
    fn a_limit_of_zero_lets_everything_through() {
        let throttle = AuthThrottle::new();
        for _ in 0..1000 {
            assert!(throttle
                .check_address(address(), AttemptClass::Credential, 0)
                .is_ok());
        }
    }

    #[test]
    fn requests_are_refused_once_the_bucket_is_empty() {
        let throttle = AuthThrottle::new();
        for _ in 0..3 {
            assert!(throttle
                .check_address(address(), AttemptClass::Credential, 3)
                .is_ok());
        }

        let wait = throttle
            .check_address(address(), AttemptClass::Credential, 3)
            .expect_err("fourth is refused");
        assert!(wait <= WINDOW);
    }

    #[test]
    fn the_wait_is_the_time_to_earn_one_token_not_a_whole_window() {
        // The fixed window this replaced told a caller to wait out the rest of
        // the minute; a bucket can serve them again as soon as one token has
        // been earned back.
        let throttle = AuthThrottle::new();
        for _ in 0..60 {
            assert!(throttle
                .check_address(address(), AttemptClass::Credential, 60)
                .is_ok());
        }
        let wait = throttle
            .check_address(address(), AttemptClass::Credential, 60)
            .expect_err("bucket is empty");
        assert!(wait < Duration::from_secs(2), "expected about a second, got {wait:?}");
    }

    #[test]
    fn a_full_minutes_allowance_may_be_spent_at_once() {
        // A page-reload flurry should not be refused just because it arrived
        // together.
        let throttle = AuthThrottle::new();
        for _ in 0..30 {
            assert!(throttle
                .check_address(address(), AttemptClass::Lookup, 30)
                .is_ok());
        }
    }

    #[test]
    fn each_address_gets_its_own_allowance() {
        let throttle = AuthThrottle::new();
        let other: IpAddr = "198.51.100.9".parse().expect("valid address");
        assert!(throttle
            .check_address(address(), AttemptClass::Credential, 1)
            .is_ok());
        assert!(throttle
            .check_address(address(), AttemptClass::Credential, 1)
            .is_err());
        assert!(throttle
            .check_address(other, AttemptClass::Credential, 1)
            .is_ok());
    }

    #[test]
    fn cheap_lookups_cannot_exhaust_the_allowance_guarding_sign_in() {
        // The whole point of splitting the classes: unlocking a vault spends a
        // lookup, and that must not be able to lock the user out of signing in.
        let throttle = AuthThrottle::new();
        assert!(throttle
            .check_address(address(), AttemptClass::Lookup, 1)
            .is_ok());
        assert!(throttle
            .check_address(address(), AttemptClass::Lookup, 1)
            .is_err());
        assert!(throttle
            .check_address(address(), AttemptClass::Credential, 1)
            .is_ok());
    }

    #[test]
    fn an_account_locks_after_the_threshold_and_reports_the_wait() {
        let throttle = AuthThrottle::new();
        assert!(throttle
            .lockout_remaining("alice", CredentialKind::Password)
            .is_none());

        for _ in 0..3 {
            throttle.record_failure("alice", CredentialKind::Password, 3, 15);
        }

        let remaining = throttle
            .lockout_remaining("alice", CredentialKind::Password)
            .expect("locked");
        assert!(remaining > Duration::from_secs(14 * 60));
    }

    #[test]
    fn casing_cannot_be_used_to_get_a_second_allowance() {
        let throttle = AuthThrottle::new();
        throttle.record_failure("Alice", CredentialKind::Password, 2, 15);
        throttle.record_failure("alice", CredentialKind::Password, 2, 15);
        assert!(throttle
            .lockout_remaining("ALICE", CredentialKind::Password)
            .is_some());
    }

    #[test]
    fn a_successful_login_clears_the_counter() {
        let throttle = AuthThrottle::new();
        throttle.record_failure("alice", CredentialKind::Password, 3, 15);
        throttle.record_failure("alice", CredentialKind::Password, 3, 15);
        throttle.record_success("alice", CredentialKind::Password);
        throttle.record_failure("alice", CredentialKind::Password, 3, 15);
        assert!(throttle
            .lockout_remaining("alice", CredentialKind::Password)
            .is_none());
    }

    #[test]
    fn a_threshold_of_zero_never_locks_anyone_out() {
        let throttle = AuthThrottle::new();
        for _ in 0..50 {
            throttle.record_failure("alice", CredentialKind::Password, 0, 15);
        }
        assert!(throttle
            .lockout_remaining("alice", CredentialKind::Password)
            .is_none());
    }
}
