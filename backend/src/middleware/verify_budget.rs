//! Bounds the CPU one instance will spend verifying credentials.
//!
//! Password verification is Argon2id — deliberately expensive, and the reason a
//! stolen database is not a list of passwords. It also means every sign-in
//! attempt costs the server tens of milliseconds of a core and 64 MiB of
//! memory, and an attacker chooses how many attempts arrive.
//!
//! The per-address throttle does not bound this. It is keyed on the address,
//! which is exactly what an attacker with a botnet spreads across: ten thousand
//! sources each staying politely under the limit still arrive as ten thousand
//! Argon2 hashes. This is the control that does not care where the requests
//! came from.
//!
//! It also gets the hashing off the async executor. Argon2 was being run
//! directly inside the request handler, so each verification blocked a runtime
//! worker outright — with enough concurrent sign-ins the server stops serving
//! anything at all, including the requests that are not sign-ins.

use std::{sync::Arc, time::Duration};

use tokio::sync::{Semaphore, TryAcquireError};

use crate::error::AppError;

/// How long a caller waits for a slot before being turned away.
///
/// Long enough to absorb a burst that is merely badly timed, short enough that
/// queued attackers do not accumulate into the memory problem the semaphore
/// exists to prevent.
const QUEUE_FOR: Duration = Duration::from_millis(750);

/// A permit pool over the machine's cores.
#[derive(Clone)]
pub struct VerifyBudget {
    permits: Arc<Semaphore>,
    limit: usize,
}

impl VerifyBudget {
    /// `concurrency` of 0 means "decide from the hardware".
    ///
    /// The default is the number of cores: Argon2 is configured with
    /// `p_cost = 4` but the reference implementation runs the lanes on one
    /// thread, so one verification saturates roughly one core.
    pub fn new(concurrency: usize) -> Self {
        let limit = if concurrency > 0 {
            concurrency
        } else {
            std::thread::available_parallelism()
                .map(|value| value.get())
                .unwrap_or(4)
        };

        Self {
            permits: Arc::new(Semaphore::new(limit)),
            limit,
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Runs `work` on a blocking thread, holding a permit for its duration.
    ///
    /// Returns `TooManyRequests` rather than queueing without bound: a caller
    /// told to come back is cheap, while a caller held open is 64 MiB of
    /// Argon2 working memory waiting to be allocated.
    pub async fn run<F, T>(&self, work: F) -> Result<T, AppError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let permit = match self.permits.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(TryAcquireError::NoPermits) => {
                match tokio::time::timeout(QUEUE_FOR, self.permits.clone().acquire_owned()).await {
                    Ok(Ok(permit)) => permit,
                    // Elapsed, or the semaphore was closed during shutdown.
                    _ => {
                        tracing::warn!(
                            limit = self.limit,
                            "credential verification is saturated; turning a request away"
                        );
                        return Err(AppError::TooManyRequests(
                            "the server is busy verifying sign-ins; please try again".to_string(),
                            1,
                        ));
                    }
                }
            }
            Err(TryAcquireError::Closed) => {
                return Err(AppError::Internal("verify budget closed".to_string()))
            }
        };

        let result = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        })
        .await
        .map_err(|error| AppError::Internal(format!("verification task failed: {error}")))?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_concurrency_is_honoured() {
        assert_eq!(VerifyBudget::new(3).limit(), 3);
    }

    #[test]
    fn zero_falls_back_to_the_hardware() {
        assert!(VerifyBudget::new(0).limit() >= 1);
    }

    #[tokio::test]
    async fn work_runs_and_returns_its_value() {
        let budget = VerifyBudget::new(2);
        assert_eq!(budget.run(|| 21 * 2).await.expect("runs"), 42);
    }

    #[tokio::test]
    async fn a_permit_is_released_when_the_work_finishes() {
        // A permit leak would show up as the second call hanging until the
        // queue timeout and then failing.
        let budget = VerifyBudget::new(1);
        for _ in 0..5 {
            assert!(budget.run(|| ()).await.is_ok());
        }
    }

    #[tokio::test]
    async fn a_saturated_budget_turns_callers_away_rather_than_queueing_forever() {
        let budget = VerifyBudget::new(1);
        let held = budget.permits.clone().acquire_owned().await.expect("permit");

        let refused = budget.run(|| ()).await;
        assert!(
            matches!(refused, Err(AppError::TooManyRequests(_, _))),
            "expected the caller to be turned away",
        );

        drop(held);
        assert!(budget.run(|| ()).await.is_ok(), "recovers once a slot frees");
    }
}
