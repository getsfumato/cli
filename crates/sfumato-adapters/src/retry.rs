//! Transport-level retrying for transient provider failures.
//!
//! `sfumato_core::retry` decides how long to wait; this module does the waiting
//! and owns the decorators that put it in the request path. It lives here rather
//! than in core because waiting needs the async runtime, and because the right
//! place to repeat a request is the transport — one model turn, one synthesis
//! call — never a workflow that has already spent tool calls or written files.
//!
//! Every decorator is transparent: same port, same errors, and on the last
//! attempt the provider's own error surfaces unchanged.

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use sfumato_core::{
    errors::{OperationStage, SfumatoResult},
    operation::{OperationContext, OperationEventKind},
    providers::{
        ImageGenerationProvider, ImageGenerationRequest, ImageGenerationResponse,
        SpeechGenerationProvider, SpeechGenerationRequest, SpeechGenerationResponse, TextModel,
        TextModelRequest, TextModelResponse,
    },
    retry::{RetryDecision, RetryPolicy},
};

/// Repeats one transport call while the policy says a transient failure can clear.
///
/// `attempt` builds a fresh future each time so the caller keeps ownership of its
/// request and nothing is reused across attempts. `activity` names the call in the
/// retry event a frontend shows.
pub(crate) async fn with_retry<T, F, Fut>(
    policy: RetryPolicy,
    operation: &OperationContext,
    stage: OperationStage,
    activity: &str,
    mut attempt: F,
) -> SfumatoResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = SfumatoResult<T>>,
{
    let mut number = 1;
    loop {
        // Before each attempt, not only before the first: a cancellation or
        // deadline that arrived during the backoff must stop the retry rather
        // than spend another paid call on it.
        operation.checkpoint(stage)?;
        let error = match attempt().await {
            Ok(value) => return Ok(value),
            Err(error) => error,
        };
        let decision = policy.decide(number, &error, operation.remaining(), entropy());
        let RetryDecision::RetryAfter(backoff) = decision else {
            return Err(error);
        };
        operation.emit(
            stage,
            OperationEventKind::Retry,
            [
                ("activity".to_string(), activity.to_string()),
                ("attempt".to_string(), number.to_string()),
                ("backoff_ms".to_string(), backoff.as_millis().to_string()),
                ("class".to_string(), error.class.to_string()),
            ]
            .into_iter()
            .collect(),
        );
        sleep_until_cancelled(operation, stage, backoff).await?;
        number += 1;
    }
}

/// Waits out a backoff without outliving a cancellation or the deadline.
///
/// A plain sleep would hold a 30-second backoff after the user pressed Esc, so
/// the wait races the same two signals every other adapter call observes.
async fn sleep_until_cancelled(
    operation: &OperationContext,
    stage: OperationStage,
    backoff: Duration,
) -> SfumatoResult<()> {
    let deadline = tokio::time::Instant::now() + backoff;
    loop {
        operation.checkpoint(stage)?;
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Ok(());
        }
        tokio::time::sleep((deadline - now).min(Duration::from_millis(50))).await;
    }
}

/// A varying value used to spread concurrent backoffs apart.
///
/// The wall clock rather than a random number generator: jitter only has to
/// decorrelate callers, and this needs no dependency to do it.
fn entropy() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| u64::from(since.subsec_nanos()))
}

/// A one-turn text transport that retries its own transient failures.
///
/// Wrapping the transport rather than the agent is the whole point: a retry here
/// repeats exactly one model turn, while a retry around the agent loop would
/// re-execute every tool call the turn already made.
pub(crate) struct RetryingTextModel {
    inner: Arc<dyn TextModel>,
    policy: RetryPolicy,
}

impl RetryingTextModel {
    /// Wraps a transport with the default policy.
    pub(crate) fn new(inner: Arc<dyn TextModel>) -> Self {
        Self {
            inner,
            policy: RetryPolicy::default(),
        }
    }
}

#[async_trait]
impl TextModel for RetryingTextModel {
    async fn complete(
        &self,
        request: TextModelRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<TextModelResponse> {
        with_retry(self.policy, operation, stage, "model_request", || {
            self.inner.complete(request.clone(), operation, stage)
        })
        .await
    }
}

/// A speech synthesizer that retries its own transient failures.
pub(crate) struct RetryingSpeechProvider {
    inner: Box<dyn SpeechGenerationProvider>,
    policy: RetryPolicy,
}

impl RetryingSpeechProvider {
    /// Wraps a synthesizer with the default policy.
    pub(crate) fn new(inner: Box<dyn SpeechGenerationProvider>) -> Self {
        Self {
            inner,
            policy: RetryPolicy::default(),
        }
    }
}

#[async_trait]
impl SpeechGenerationProvider for RetryingSpeechProvider {
    async fn generate_speech(
        &self,
        request: SpeechGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<SpeechGenerationResponse> {
        with_retry(self.policy, operation, stage, "speech_request", || {
            self.inner
                .generate_speech(request.clone(), operation, stage)
        })
        .await
    }
}

/// An image generator that retries its own transient failures.
pub(crate) struct RetryingImageProvider {
    inner: Box<dyn ImageGenerationProvider>,
    policy: RetryPolicy,
}

impl RetryingImageProvider {
    /// Wraps a generator with the default policy.
    pub(crate) fn new(inner: Box<dyn ImageGenerationProvider>) -> Self {
        Self {
            inner,
            policy: RetryPolicy::default(),
        }
    }
}

#[async_trait]
impl ImageGenerationProvider for RetryingImageProvider {
    async fn generate_image(
        &self,
        request: ImageGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<ImageGenerationResponse> {
        with_retry(self.policy, operation, stage, "image_request", || {
            self.inner.generate_image(request.clone(), operation, stage)
        })
        .await
    }
}

#[cfg(test)]
#[path = "../tests/unit/retry.rs"]
mod tests;
