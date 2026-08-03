//! Unit tests for interruptible, deadline-bounded command operations.

use super::*;

#[test]
fn the_process_deadline_is_recorded_once_from_the_flag() {
    // `TIMEOUT` is a `OnceLock`, so this is the only test that may write it and it
    // asserts the round-trip. Every other test passes a deadline explicitly.
    set_timeout(Some(90));
    assert_eq!(timeout(), Some(Duration::from_secs(90)));
}

#[tokio::test]
async fn an_omitted_timeout_leaves_the_operation_unbounded() {
    // The default has to stay unbounded: a long film is a legitimate multi-minute
    // operation, and inventing a deadline would abort work that was succeeding.
    let operation = interruptible_with(None);
    assert_eq!(operation.remaining(), None);
    assert!(!operation.cancellation.is_cancelled());
}

#[tokio::test]
async fn a_requested_timeout_becomes_the_operation_deadline() {
    let operation = interruptible_with(Some(Duration::from_secs(60)));
    let remaining = operation.remaining().expect("a deadline was requested");
    assert!(
        remaining <= Duration::from_secs(60) && remaining > Duration::from_secs(55),
        "the deadline should be the requested window, not a fraction of it: {remaining:?}"
    );
}

#[tokio::test]
async fn an_expired_deadline_stops_the_next_checkpoint() {
    use sfumato_core::errors::{ErrorClass, OperationStage};

    let operation = interruptible_with(Some(Duration::from_millis(20)));
    tokio::time::sleep(Duration::from_millis(60)).await;
    let error = operation
        .checkpoint(OperationStage::Draft)
        .expect_err("a passed deadline stops the operation");
    assert_eq!(error.class, ErrorClass::Cancelled);
}

#[tokio::test]
async fn cancelling_turns_the_next_checkpoint_into_an_error() {
    // This is the whole mechanism behind interrupt cleanup: the token becomes an
    // error at the next checkpoint, which unwinds and drops the artifact
    // transaction, which removes its staging directory.
    use sfumato_core::errors::{ErrorClass, OperationStage};

    let (handle, operation) =
        OperationContext::create(None, Arc::new(sfumato_core::operation::DiscardEvents));
    assert!(operation.checkpoint(OperationStage::Draft).is_ok());
    assert!(handle.cancel(), "the first cancel changes the state");
    assert!(!handle.cancel(), "a second cancel is a no-op");
    let error = operation
        .checkpoint(OperationStage::Draft)
        .expect_err("a cancelled operation stops at its next checkpoint");
    assert_eq!(error.class, ErrorClass::Cancelled);
}
