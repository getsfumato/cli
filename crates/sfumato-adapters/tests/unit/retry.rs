//! Unit tests for transport-level retrying of transient provider failures.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
};

use sfumato_core::{
    errors::{ErrorClass, ErrorCode, SfumatoError},
    providers::TextModelResponse,
    retry::RETRY_AFTER_DETAIL,
};

use super::*;

fn transient() -> SfumatoError {
    SfumatoError::new(ErrorCode::Provider, ErrorClass::Retry, "HTTP 429")
}

fn permanent() -> SfumatoError {
    SfumatoError::new(ErrorCode::Provider, ErrorClass::Permanent, "HTTP 400")
}

/// A policy with no real waiting, so the tests assert on attempts not timing.
fn fast() -> RetryPolicy {
    RetryPolicy {
        max_attempts: 4,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(2),
        multiplier: 2,
    }
}

#[tokio::test]
async fn returns_the_first_success_without_retrying() {
    let calls = AtomicU32::new(0);
    let operation = OperationContext::detached();
    let result = with_retry(fast(), &operation, OperationStage::Draft, "test", || {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Ok::<_, SfumatoError>(7) }
    })
    .await;
    assert_eq!(result.unwrap(), 7);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn recovers_when_a_later_attempt_succeeds() {
    let calls = AtomicU32::new(0);
    let operation = OperationContext::detached();
    let result = with_retry(fast(), &operation, OperationStage::Draft, "test", || {
        let attempt = calls.fetch_add(1, Ordering::SeqCst);
        async move {
            if attempt < 2 {
                Err(transient())
            } else {
                Ok("recovered")
            }
        }
    })
    .await;
    assert_eq!(result.unwrap(), "recovered");
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn surfaces_a_permanent_failure_on_the_first_attempt() {
    let calls = AtomicU32::new(0);
    let operation = OperationContext::detached();
    let result = with_retry(fast(), &operation, OperationStage::Draft, "test", || {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Err::<(), _>(permanent()) }
    })
    .await;
    assert_eq!(result.unwrap_err().class, ErrorClass::Permanent);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn gives_up_after_the_attempt_budget_and_keeps_the_provider_error() {
    let calls = AtomicU32::new(0);
    let operation = OperationContext::detached();
    let result = with_retry(fast(), &operation, OperationStage::Draft, "test", || {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Err::<(), _>(transient()) }
    })
    .await;
    let error = result.unwrap_err();
    assert_eq!(error.class, ErrorClass::Retry);
    assert_eq!(error.message, "HTTP 429");
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn stops_retrying_once_the_operation_is_cancelled() {
    let (handle, operation) =
        OperationContext::create(None, Arc::new(sfumato_core::operation::DiscardEvents));
    let calls = AtomicU32::new(0);
    let result = with_retry(fast(), &operation, OperationStage::Draft, "test", || {
        handle.cancel();
        calls.fetch_add(1, Ordering::SeqCst);
        async { Err::<(), _>(transient()) }
    })
    .await;
    assert_eq!(result.unwrap_err().class, ErrorClass::Cancelled);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn does_not_wait_out_a_provider_delay_that_outlasts_the_deadline() {
    let operation = OperationContext::create(
        Some(Duration::from_millis(200)),
        Arc::new(sfumato_core::operation::DiscardEvents),
    )
    .1;
    let calls = AtomicU32::new(0);
    let result = with_retry(
        RetryPolicy::default(),
        &operation,
        OperationStage::Draft,
        "test",
        || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err::<(), _>(transient().with_detail(RETRY_AFTER_DETAIL, "20")) }
        },
    )
    .await;
    assert_eq!(result.unwrap_err().class, ErrorClass::Retry);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a 20s delay inside a 200ms deadline is not worth waiting for"
    );
}

struct FlakyModel {
    failures: AtomicU32,
    seen: Mutex<Vec<usize>>,
}

#[async_trait]
impl TextModel for FlakyModel {
    async fn complete(
        &self,
        request: TextModelRequest,
        _: &OperationContext,
        _: OperationStage,
    ) -> SfumatoResult<TextModelResponse> {
        self.seen.lock().unwrap().push(request.messages.len());
        if self.failures.fetch_sub(1, Ordering::SeqCst) > 0 {
            return Err(transient());
        }
        Ok(TextModelResponse {
            content: Some("done".to_string()),
            tool_calls: Vec::new(),
        })
    }
}

#[tokio::test]
async fn the_text_decorator_resends_the_same_transcript() {
    let model = Arc::new(FlakyModel {
        failures: AtomicU32::new(2),
        seen: Mutex::new(Vec::new()),
    });
    let retrying = RetryingTextModel {
        inner: model.clone(),
        policy: fast(),
    };
    let response = retrying
        .complete(
            TextModelRequest {
                messages: vec![sfumato_core::providers::ModelMessage::User("hi".into())],
                tools: Vec::new(),
            },
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .expect("the third attempt succeeds");
    assert_eq!(response.content.as_deref(), Some("done"));
    assert_eq!(
        *model.seen.lock().unwrap(),
        vec![1, 1, 1],
        "every attempt sends the identical transcript"
    );
}
