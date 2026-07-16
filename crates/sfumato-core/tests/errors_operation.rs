#[path = "../src/errors.rs"]
mod errors;
#[path = "../src/operation.rs"]
mod operation;

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use errors::{ErrorClass, ErrorCode, OperationStage, SfumatoError};
use operation::{
    CancellationHandle, DiscardEvents, EventDelivery, EventSink, EventSinkError, OperationContext,
    OperationEvent, OperationEventKind,
};
use sfumato_domain::JobId;

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<OperationEvent>>,
}

impl EventSink for RecordingSink {
    fn try_emit(&self, event: OperationEvent) -> Result<(), EventSinkError> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

struct RejectingSink(EventSinkError);

impl EventSink for RejectingSink {
    fn try_emit(&self, _: OperationEvent) -> Result<(), EventSinkError> {
        Err(self.0)
    }
}

#[test]
fn error_classes_expose_recovery_semantics() {
    for class in [
        ErrorClass::Retry,
        ErrorClass::ContextLimit,
        ErrorClass::InvalidOutput,
        ErrorClass::Unavailable,
    ] {
        assert!(class.is_retryable(), "{class} should permit recovery");
    }
    assert!(!ErrorClass::Cancelled.is_retryable());
    assert!(!ErrorClass::Permanent.is_retryable());
}

#[test]
fn typed_error_preserves_stable_code_stage_and_sanitized_details() {
    let error = SfumatoError::new(
        ErrorCode::Provider,
        ErrorClass::ContextLimit,
        "model context is too large",
    )
    .at_stage(OperationStage::Draft)
    .with_detail("model", "local-text");

    assert_eq!(error.code.as_str(), "provider");
    assert_eq!(error.class.as_str(), "context_limit");
    assert_eq!(error.stage, Some(OperationStage::Draft));
    assert_eq!(error.details["model"], "local-text");
    assert!(error.retryable);
    assert!(error.is_retryable());
    assert_eq!(
        error.to_string(),
        "provider during draft: model context is too large"
    );
}

#[test]
fn cancellation_is_cloneable_one_way_and_idempotent() {
    let (handle, token) = CancellationHandle::new_pair();
    let cloned = token.clone();

    assert!(!token.is_cancelled());
    assert!(handle.cancel());
    assert!(!handle.cancel());
    assert!(handle.is_cancelled());
    assert!(token.is_cancelled());
    assert!(cloned.is_cancelled());

    let error = cloned.checkpoint(Some(OperationStage::Review)).unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.class, ErrorClass::Cancelled);
    assert_eq!(error.stage, Some(OperationStage::Review));
    assert!(!error.is_retryable());
}

#[test]
fn cancellation_code_and_class_are_normalized_together() {
    let by_code = SfumatoError::new(
        ErrorCode::Cancelled,
        ErrorClass::Permanent,
        "deadline elapsed",
    );
    let by_class = SfumatoError::new(
        ErrorCode::Provider,
        ErrorClass::Cancelled,
        "request cancelled",
    );

    for error in [by_code, by_class] {
        assert_eq!(error.code, ErrorCode::Cancelled);
        assert_eq!(error.class, ErrorClass::Cancelled);
        assert!(!error.retryable);
    }
}

#[test]
fn operation_checkpoint_distinguishes_deadline_from_explicit_cancellation() {
    let (handle, token) = CancellationHandle::new_pair();
    let context = OperationContext::new(
        JobId::new("job-deadline").unwrap(),
        Some(Instant::now() - Duration::from_millis(1)),
        token,
        Arc::new(RecordingSink::default()),
    );

    let deadline = context.checkpoint(OperationStage::Render).unwrap_err();
    assert_eq!(deadline.code, ErrorCode::Cancelled);
    assert_eq!(deadline.details["reason"], "deadline_exceeded");

    handle.cancel();
    let cancelled = context.checkpoint(OperationStage::Render).unwrap_err();
    assert_eq!(cancelled.message, "operation cancelled");
    assert!(cancelled.details.is_empty());
}

#[test]
fn cloned_contexts_emit_one_monotonic_job_scoped_sequence() {
    let (_, token) = CancellationHandle::new_pair();
    let sink = Arc::new(RecordingSink::default());
    let context =
        OperationContext::new(JobId::new("job-events").unwrap(), None, token, sink.clone());

    assert_eq!(
        context.emit(
            OperationStage::Draft,
            OperationEventKind::Started,
            BTreeMap::new(),
        ),
        EventDelivery::Delivered
    );
    assert_eq!(
        context.clone().emit(
            OperationStage::Draft,
            OperationEventKind::Completed,
            BTreeMap::from([("model".to_string(), "local-text".to_string())]),
        ),
        EventDelivery::Delivered
    );

    let events = sink.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].job_id.as_str(), "job-events");
    assert_eq!(events[0].sequence, 1);
    assert_eq!(events[1].sequence, 2);
    assert_eq!(events[1].fields["model"], "local-text");
}

#[test]
fn event_backpressure_is_nonblocking_and_never_becomes_operation_failure() {
    let (_, token) = CancellationHandle::new_pair();
    let full = OperationContext::new(
        JobId::new("job-full").unwrap(),
        None,
        token.clone(),
        Arc::new(RejectingSink(EventSinkError::Full)),
    );
    let closed = OperationContext::new(
        JobId::new("job-closed").unwrap(),
        None,
        token,
        Arc::new(RejectingSink(EventSinkError::Closed)),
    );

    assert_eq!(
        full.emit(
            OperationStage::Resolve,
            OperationEventKind::Progress,
            BTreeMap::new(),
        ),
        EventDelivery::DroppedFull
    );
    assert_eq!(
        closed.emit(
            OperationStage::Resolve,
            OperationEventKind::Warning,
            BTreeMap::new(),
        ),
        EventDelivery::DroppedClosed
    );
    assert!(full.checkpoint(OperationStage::Resolve).is_ok());
    assert!(closed.checkpoint(OperationStage::Resolve).is_ok());
}

#[test]
fn relative_deadline_reports_remaining_time_without_runtime_dependencies() {
    let (_, token) = CancellationHandle::new_pair();
    let context = OperationContext::with_timeout(
        JobId::new("job-timeout").unwrap(),
        Duration::from_secs(1),
        token,
        Arc::new(RecordingSink::default()),
    );

    let remaining = context.remaining().unwrap();
    assert!(remaining > Duration::ZERO);
    assert!(remaining <= Duration::from_secs(1));
}

#[test]
fn discard_sink_accepts_events_without_storage_or_backpressure() {
    let (_, token) = CancellationHandle::new_pair();
    let context = OperationContext::new(
        JobId::new("job-discard").unwrap(),
        None,
        token,
        Arc::new(DiscardEvents),
    );

    assert_eq!(
        context.emit(
            OperationStage::Publish,
            OperationEventKind::Completed,
            BTreeMap::new(),
        ),
        EventDelivery::Delivered
    );
}
