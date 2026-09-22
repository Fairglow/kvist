//! Bounded retry policy with exponential backoff for transient model failures.
//!
//! Long-running, generation-bound model turns can trip a per-turn deadline or
//! drop mid-stream (a network reset, a stalled provider, a transient 5xx). When
//! a failure is *temporal* — reported by [`agent_runtime::Error::is_retryable`]
//! — replaying the turn recovers it: each attempt is a fresh request that gets a
//! new deadline. This module decides how many attempts to make and how long to
//! wait between them, growing the wait exponentially up to a cap. The backoff
//! schedule is deterministic (no jitter) so behavior is reproducible.

use std::time::Duration;

/// Default total attempts for one turn, including the initial try.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
/// Default delay before the first retry.
pub const DEFAULT_RETRY_BASE_DELAY: Duration = Duration::from_secs(2);
/// Default upper bound on any single backoff delay.
pub const DEFAULT_RETRY_MAX_DELAY: Duration = Duration::from_secs(30);

/// A bounded retry policy: how many attempts to make for one turn and how long
/// to wait between them.
///
/// `max_attempts` counts the initial attempt plus retries, so `max_attempts ==
/// 1` disables retrying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts for one turn, including the first (`>= 1`).
    pub max_attempts: u32,
    /// Delay before the first retry; the base for exponential growth.
    pub base_delay: Duration,
    /// Upper bound on any single backoff delay.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            base_delay: DEFAULT_RETRY_BASE_DELAY,
            max_delay: DEFAULT_RETRY_MAX_DELAY,
        }
    }
}

impl RetryPolicy {
    /// Builds a policy, clamping invalid fields to sane bounds:
    /// `max_attempts` at least 1, `base_delay` within `[1ms, max_delay]`, and
    /// `max_delay` at least `base_delay`.
    pub fn new(max_attempts: u32, base_delay: Duration, max_delay: Duration) -> Self {
        // `max_delay` is the hard ceiling (never below 1ms); `base_delay` is then
        // clamped so it can never exceed that ceiling, and is floored at 1ms so a
        // zero base delay still schedules a finite, non-zero backoff.
        let max_delay = max_delay.max(Duration::from_millis(1));
        let base_delay = base_delay.min(max_delay).max(Duration::from_millis(1));
        Self {
            max_attempts: max_attempts.max(1),
            base_delay,
            max_delay,
        }
    }

    /// The backoff delay that precedes attempt `attempt`, which is 2-based: the
    /// delay before the second (first retry) attempt is the base delay, before
    /// the third it doubles, and so on, capped at `max_delay`.
    ///
    /// Computed by repeated doubling with an early return on the cap, so it can
    /// never overflow `Duration`.
    pub fn backoff_delay(&self, attempt: u32) -> Duration {
        // `attempt == 2` is the first retry (exponent 0). Clamp defensively for
        // any smaller or absurdly large value.
        let exponent = attempt.saturating_sub(2).min(32);
        let mut delay = self.base_delay;
        for _ in 0..exponent {
            match delay.checked_mul(2) {
                Some(next) if next < self.max_delay => delay = next,
                _ => return self.max_delay,
            }
        }
        delay.min(self.max_delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    #[test]
    fn default_policy_is_triple_attempt_with_growing_backoff() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts, DEFAULT_MAX_ATTEMPTS);
        assert_eq!(policy.base_delay, DEFAULT_RETRY_BASE_DELAY);
        assert_eq!(policy.max_delay, DEFAULT_RETRY_MAX_DELAY);
    }

    #[test]
    fn backoff_grows_exponentially_then_caps() {
        let policy = RetryPolicy::new(5, secs(2), secs(30));
        assert_eq!(policy.backoff_delay(2), secs(2));
        assert_eq!(policy.backoff_delay(3), secs(4));
        assert_eq!(policy.backoff_delay(4), secs(8));
        assert_eq!(policy.backoff_delay(5), secs(16));
        assert_eq!(policy.backoff_delay(6), secs(30));
        assert_eq!(policy.backoff_delay(100), secs(30));
    }

    #[test]
    fn backoff_never_overflows_for_extreme_attempts() {
        let policy = RetryPolicy::new(2, Duration::from_nanos(1), secs(3600));
        assert!(policy.backoff_delay(10_000) <= secs(3600));
        assert!(!policy.backoff_delay(10_000).is_zero());
    }

    #[test]
    fn invalid_fields_are_clamped_to_sane_bounds() {
        let policy = RetryPolicy::new(0, Duration::ZERO, Duration::from_secs(5));
        assert_eq!(policy.max_attempts, 1);
        assert_eq!(policy.base_delay, Duration::from_millis(1));
        assert_eq!(policy.max_delay, Duration::from_secs(5));

        let policy = RetryPolicy::new(3, secs(60), secs(10));
        assert_eq!(policy.base_delay, secs(10));
        assert_eq!(policy.max_delay, secs(10));
        assert_eq!(policy.backoff_delay(3), secs(10));
    }

    #[test]
    fn single_attempt_policy_has_no_retry() {
        let policy = RetryPolicy::new(1, secs(1), secs(5));
        assert_eq!(policy.max_attempts, 1);
    }
}
