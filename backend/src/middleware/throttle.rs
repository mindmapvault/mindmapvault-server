//! Per-address request throttling and failed-login lockout for the auth routes.
//!
//! Kept in process memory on purpose. The state is small, worthless to persist
//! (a restart clearing it is an acceptable trade), and putting it in Postgres
//! would mean a write on every failed password attempt — exactly the traffic an
//! attacker controls.
//!
//! Two separate mechanisms, because they answer different attacks:
//!
//! * the **address counter** limits how fast one caller can hit the auth routes
//!   at all, which is what stops a credential-stuffing run;
//! * the **failed-login counter** parks one account after repeated wrong
//!   passwords, which is what stops a slow guess against a known username.
//!
//! The second one can be turned against a user: anyone who knows a username can
//! keep it locked by failing logins on purpose. That is why the lockout is
//! measured in minutes rather than being permanent, and why it is a separate
//! knob an operator can switch off. The admin `is_locked` flag is untouched by
//! any of this — that one is a deliberate act by a person.

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

/// How long a counted window lasts. The configured limit is "requests per
/// minute", so the window is a minute.
const WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy)]
struct Window {
    started_at: Instant,
    hits: u32,
}

#[derive(Debug, Clone, Copy)]
struct Failures {
    count: i32,
    last_seen: Instant,
    locked_until: Option<Instant>,
}

#[derive(Default)]
struct ThrottleState {
    addresses: HashMap<IpAddr, Window>,
    failures: HashMap<String, Failures>,
}

#[derive(Default)]
pub struct AuthThrottle {
    state: Mutex<ThrottleState>,
}

impl AuthThrottle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts one auth request from `address`. Returns how long the caller has
    /// to wait when the limit is already used up.
    ///
    /// `limit` of 0 disables the check, which is how an operator opts out.
    pub fn check_address(&self, address: IpAddr, limit: i32) -> Result<(), Duration> {
        if limit <= 0 {
            return Ok(());
        }

        let now = Instant::now();
        let mut state = self.lock();
        let window = state.addresses.entry(address).or_insert(Window {
            started_at: now,
            hits: 0,
        });

        if now.duration_since(window.started_at) >= WINDOW {
            *window = Window {
                started_at: now,
                hits: 0,
            };
        }

        if window.hits >= limit as u32 {
            return Err(WINDOW.saturating_sub(now.duration_since(window.started_at)));
        }

        window.hits += 1;
        Ok(())
    }

    /// How long `username` stays locked out after too many wrong passwords.
    pub fn lockout_remaining(&self, username: &str) -> Option<Duration> {
        let now = Instant::now();
        let mut state = self.lock();
        let key = normalize(username);
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

    /// Records a wrong password. Locks the account once `threshold` is reached.
    pub fn record_failure(&self, username: &str, threshold: i32, lockout_minutes: i32) {
        if threshold <= 0 {
            return;
        }

        let now = Instant::now();
        let lockout = Duration::from_secs(lockout_minutes.max(1) as u64 * 60);
        let mut state = self.lock();
        let record = state.failures.entry(normalize(username)).or_insert(Failures {
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

    /// Clears the counter after a successful login.
    pub fn record_success(&self, username: &str) {
        self.lock().failures.remove(&normalize(username));
    }

    /// Drops entries nothing is waiting on. Called from a background timer so
    /// the maps do not grow with every address that ever touched the server.
    pub fn prune(&self) {
        let now = Instant::now();
        let mut state = self.lock();
        state
            .addresses
            .retain(|_, window| now.duration_since(window.started_at) < WINDOW);
        state.failures.retain(|_, record| {
            record
                .locked_until
                .is_some_and(|until| until > now)
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
            assert!(throttle.check_address(address(), 0).is_ok());
        }
    }

    #[test]
    fn requests_are_refused_once_the_window_is_used_up() {
        let throttle = AuthThrottle::new();
        for _ in 0..3 {
            assert!(throttle.check_address(address(), 3).is_ok());
        }

        let wait = throttle.check_address(address(), 3).expect_err("fourth is refused");
        assert!(wait <= WINDOW);
    }

    #[test]
    fn each_address_gets_its_own_allowance() {
        let throttle = AuthThrottle::new();
        let other: IpAddr = "198.51.100.9".parse().expect("valid address");
        assert!(throttle.check_address(address(), 1).is_ok());
        assert!(throttle.check_address(address(), 1).is_err());
        assert!(throttle.check_address(other, 1).is_ok());
    }

    #[test]
    fn an_account_locks_after_the_threshold_and_reports_the_wait() {
        let throttle = AuthThrottle::new();
        assert!(throttle.lockout_remaining("alice").is_none());

        for _ in 0..3 {
            throttle.record_failure("alice", 3, 15);
        }

        let remaining = throttle.lockout_remaining("alice").expect("locked");
        assert!(remaining > Duration::from_secs(14 * 60));
    }

    #[test]
    fn casing_cannot_be_used_to_get_a_second_allowance() {
        let throttle = AuthThrottle::new();
        throttle.record_failure("Alice", 2, 15);
        throttle.record_failure("alice", 2, 15);
        assert!(throttle.lockout_remaining("ALICE").is_some());
    }

    #[test]
    fn a_successful_login_clears_the_counter() {
        let throttle = AuthThrottle::new();
        throttle.record_failure("alice", 3, 15);
        throttle.record_failure("alice", 3, 15);
        throttle.record_success("alice");
        throttle.record_failure("alice", 3, 15);
        assert!(throttle.lockout_remaining("alice").is_none());
    }

    #[test]
    fn a_threshold_of_zero_never_locks_anyone_out() {
        let throttle = AuthThrottle::new();
        for _ in 0..50 {
            throttle.record_failure("alice", 0, 15);
        }
        assert!(throttle.lockout_remaining("alice").is_none());
    }
}
