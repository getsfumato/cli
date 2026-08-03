//! Bounded backoff policy for transient provider failures.
//!
//! [`ErrorClass`] already says whether repeating an operation can help. What it
//! deliberately does not say is *when* — its own documentation leaves the
//! strategy to the caller. This module is that strategy for the two classes an
//! identical retry can actually resolve, [`ErrorClass::Retry`] and
//! [`ErrorClass::Unavailable`]: a rate limit or an overloaded dependency clears
//! on its own, so the same request sent later is the whole recovery.
//!
//! The other retryable classes are excluded on purpose.
//! [`ErrorClass::ContextLimit`] needs compaction and [`ErrorClass::InvalidOutput`]
//! needs corrective feedback; resending the identical request would burn a paid
//! call to fail the same way. Those already have their own loops.
//!
//! Deliberately free of any timer. The policy decides *how long* to wait and the
//! adapter layer, which owns the async runtime, does the waiting — that keeps
//! backoff decisions unit-testable without a clock and keeps this crate
//! runtime-neutral.

use std::time::Duration;

use crate::errors::{ErrorClass, SfumatoError};

/// Detail key an adapter uses to pass a provider's `Retry-After` through.
///
/// A provider that names a delay knows better than any local curve, so the value
/// is carried on the error rather than discarded at the HTTP boundary.
pub const RETRY_AFTER_DETAIL: &str = "retry_after_seconds";

/// Attempts made before a transient failure is surfaced.
const DEFAULT_MAX_ATTEMPTS: u32 = 4;
/// Wait before the second attempt.
const DEFAULT_INITIAL_BACKOFF: Duration = Duration::from_millis(500);
/// Ceiling for one wait, including a provider-named delay.
///
/// A provider asking for ten minutes is asking for longer than any interactive
/// generation should sit silent; past this the failure is worth surfacing so the
/// caller can decide.
const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Growth factor applied to each successive wait.
const DEFAULT_MULTIPLIER: u32 = 2;
/// Time an attempt is assumed to need before the deadline is worth spending on it.
///
/// Without it a retry can be scheduled inside a deadline that expires while the
/// request is in flight, which spends a paid call to return the deadline error
/// anyway.
const MIN_ATTEMPT_BUDGET: Duration = Duration::from_secs(1);

/// What to do after one transient failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryDecision {
    /// Wait this long, then repeat the identical request.
    RetryAfter(Duration),
    /// Surface the failure: it is permanent, out of attempts, or out of time.
    Surface,
}

/// Bounded exponential backoff with jitter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    /// Total attempts, including the first. One disables retrying.
    pub max_attempts: u32,
    /// Wait before the second attempt.
    pub initial_backoff: Duration,
    /// Ceiling for any single wait.
    pub max_backoff: Duration,
    /// Growth factor between successive waits.
    pub multiplier: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            initial_backoff: DEFAULT_INITIAL_BACKOFF,
            max_backoff: DEFAULT_MAX_BACKOFF,
            multiplier: DEFAULT_MULTIPLIER,
        }
    }
}

impl RetryPolicy {
    /// A policy that never retries, for callers that own their own recovery.
    pub const fn never() -> Self {
        Self {
            max_attempts: 1,
            initial_backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
            multiplier: 1,
        }
    }

    /// Decides what to do after `attempt` failed with `error`.
    ///
    /// `attempt` is one-based. `remaining` is the operation's remaining deadline,
    /// or `None` when it has none; a wait that would leave no room to run the
    /// next attempt surfaces the failure instead. `entropy` spreads concurrent
    /// callers apart — an adapter passes something varying such as the current
    /// nanosecond, and a test passes a fixed value to get a fixed wait.
    pub fn decide(
        &self,
        attempt: u32,
        error: &SfumatoError,
        remaining: Option<Duration>,
        entropy: u64,
    ) -> RetryDecision {
        if !is_transient(error.class) || attempt >= self.max_attempts {
            return RetryDecision::Surface;
        }
        let backoff = retry_after(error)
            .unwrap_or_else(|| self.jittered(self.backoff_for(attempt), entropy))
            .min(self.max_backoff);
        match remaining {
            // The wait plus one attempt has to fit, or the retry only delays the
            // deadline error it is going to hit anyway.
            Some(remaining) if remaining < backoff.saturating_add(MIN_ATTEMPT_BUDGET) => {
                RetryDecision::Surface
            }
            _ => RetryDecision::RetryAfter(backoff),
        }
    }

    /// The undecorated exponential wait for a one-based attempt number.
    fn backoff_for(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1);
        self.multiplier
            .checked_pow(exponent)
            .and_then(|factor| self.initial_backoff.checked_mul(factor))
            .unwrap_or(self.max_backoff)
            .min(self.max_backoff)
    }

    /// Spreads a wait across the second half of its window.
    ///
    /// Half fixed, half jittered rather than full jitter: the fixed half keeps
    /// backoff genuinely growing, and the jittered half stops several callers
    /// that hit the same rate limit from returning together and re-triggering it.
    fn jittered(&self, backoff: Duration, entropy: u64) -> Duration {
        let nanos = u64::try_from(backoff.as_nanos()).unwrap_or(u64::MAX);
        let half = nanos / 2;
        let spread = if half == 0 { 0 } else { entropy % (half + 1) };
        Duration::from_nanos(half.saturating_add(spread))
    }
}

/// Returns whether an identical repeat of the request can resolve the failure.
pub const fn is_transient(class: ErrorClass) -> bool {
    matches!(class, ErrorClass::Retry | ErrorClass::Unavailable)
}

/// Reads a provider-named delay off an error's details.
///
/// Accepts the delay-seconds form of `Retry-After`. The HTTP-date form is
/// ignored rather than parsed: it needs a wall clock this crate does not have,
/// and falling back to the local curve is a correct answer for it.
fn retry_after(error: &SfumatoError) -> Option<Duration> {
    error
        .details
        .get(RETRY_AFTER_DETAIL)?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map(Duration::from_secs_f64)
}

#[cfg(test)]
#[path = "../tests/unit/retry.rs"]
mod tests;
