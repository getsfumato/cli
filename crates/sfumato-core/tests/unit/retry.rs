//! Unit tests for bounded transient-failure backoff.

use super::*;
use crate::errors::ErrorCode;

fn error(class: ErrorClass) -> SfumatoError {
    SfumatoError::new(ErrorCode::Provider, class, "provider failed")
}

#[test]
fn surfaces_permanent_failures_without_waiting() {
    let policy = RetryPolicy::default();
    assert_eq!(
        policy.decide(1, &error(ErrorClass::Permanent), None, 0),
        RetryDecision::Surface
    );
}

#[test]
fn surfaces_classes_that_need_their_own_recovery() {
    let policy = RetryPolicy::default();
    for class in [ErrorClass::ContextLimit, ErrorClass::InvalidOutput] {
        assert_eq!(
            policy.decide(1, &error(class), None, 0),
            RetryDecision::Surface,
            "{class} needs corrective recovery, not an identical repeat"
        );
    }
}

#[test]
fn retries_rate_limits_and_unavailable_dependencies() {
    let policy = RetryPolicy::default();
    for class in [ErrorClass::Retry, ErrorClass::Unavailable] {
        assert!(
            matches!(
                policy.decide(1, &error(class), None, 0),
                RetryDecision::RetryAfter(_)
            ),
            "{class} clears on its own and is worth repeating"
        );
    }
}

#[test]
fn backoff_grows_and_stops_at_the_ceiling() {
    let policy = RetryPolicy {
        max_attempts: 8,
        initial_backoff: Duration::from_secs(1),
        max_backoff: Duration::from_secs(4),
        multiplier: 2,
    };
    let waits = (1..=5)
        .map(|attempt| policy.backoff_for(attempt))
        .collect::<Vec<_>>();
    assert_eq!(
        waits,
        vec![
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(4),
            Duration::from_secs(4),
            Duration::from_secs(4),
        ]
    );
}

#[test]
fn jitter_stays_within_the_second_half_of_the_window() {
    let policy = RetryPolicy::default();
    let backoff = Duration::from_secs(2);
    for entropy in [0, 1, 7, u64::MAX, 12_345_678_901] {
        let jittered = policy.jittered(backoff, entropy);
        assert!(
            jittered >= backoff / 2 && jittered <= backoff,
            "{jittered:?} left the window for entropy {entropy}"
        );
    }
}

#[test]
fn stops_after_the_last_attempt() {
    let policy = RetryPolicy {
        max_attempts: 3,
        ..RetryPolicy::default()
    };
    let error = error(ErrorClass::Retry);
    assert!(matches!(
        policy.decide(2, &error, None, 0),
        RetryDecision::RetryAfter(_)
    ));
    assert_eq!(policy.decide(3, &error, None, 0), RetryDecision::Surface);
}

#[test]
fn honours_a_provider_named_delay() {
    let policy = RetryPolicy::default();
    let error = error(ErrorClass::Retry).with_detail(RETRY_AFTER_DETAIL, "7");
    assert_eq!(
        policy.decide(1, &error, None, 999),
        RetryDecision::RetryAfter(Duration::from_secs(7))
    );
}

#[test]
fn caps_a_provider_named_delay_at_the_ceiling() {
    let policy = RetryPolicy::default();
    let error = error(ErrorClass::Retry).with_detail(RETRY_AFTER_DETAIL, "600");
    assert_eq!(
        policy.decide(1, &error, None, 0),
        RetryDecision::RetryAfter(DEFAULT_MAX_BACKOFF)
    );
}

#[test]
fn ignores_an_unparseable_retry_after() {
    let policy = RetryPolicy::default();
    let error = error(ErrorClass::Retry).with_detail(RETRY_AFTER_DETAIL, "Wed, 21 Oct 2015");
    let RetryDecision::RetryAfter(wait) = policy.decide(1, &error, None, 0) else {
        panic!("an unparseable delay should fall back to the local curve");
    };
    assert!(wait <= DEFAULT_INITIAL_BACKOFF);
}

#[test]
fn surfaces_when_the_wait_would_outlast_the_deadline() {
    let policy = RetryPolicy::default();
    let error = error(ErrorClass::Retry).with_detail(RETRY_AFTER_DETAIL, "20");
    assert_eq!(
        policy.decide(1, &error, Some(Duration::from_secs(5)), 0),
        RetryDecision::Surface
    );
    assert!(matches!(
        policy.decide(1, &error, Some(Duration::from_secs(60)), 0),
        RetryDecision::RetryAfter(_)
    ));
}

#[test]
fn never_disables_retrying() {
    assert_eq!(
        RetryPolicy::never().decide(1, &error(ErrorClass::Retry), None, 0),
        RetryDecision::Surface
    );
}
