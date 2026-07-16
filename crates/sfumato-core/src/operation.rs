//! Runtime-neutral operation lifecycle, cancellation, and event contracts.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sfumato_domain::JobId;

use crate::errors::{OperationStage, SfumatoError, SfumatoResult};

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

/// Sending side of a cooperative, one-way cancellation signal.
#[derive(Clone, Debug)]
pub struct CancellationHandle {
    cancelled: Arc<AtomicBool>,
}

impl CancellationHandle {
    /// Creates a handle/token pair sharing one cancellation state.
    pub fn new_pair() -> (Self, CancellationToken) {
        let cancelled = Arc::new(AtomicBool::new(false));
        (
            Self {
                cancelled: Arc::clone(&cancelled),
            },
            CancellationToken { cancelled },
        )
    }

    /// Requests cancellation and returns whether this call changed the state.
    pub fn cancel(&self) -> bool {
        !self.cancelled.swap(true, Ordering::AcqRel)
    }

    /// Returns whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Receiving side of a cooperative, one-way cancellation signal.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Returns whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Returns a typed cancellation error when cancellation was requested.
    pub fn checkpoint(&self, stage: Option<OperationStage>) -> SfumatoResult<()> {
        if self.is_cancelled() {
            Err(SfumatoError::cancelled(stage))
        } else {
            Ok(())
        }
    }
}

/// Stable kind of operation event sent to a presentation layer.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationEventKind {
    /// A workflow stage started.
    Started,
    /// A workflow stage reported intermediate progress.
    Progress,
    /// A bounded retry or recovery attempt started.
    Retry,
    /// A non-fatal condition should be shown to the caller.
    Warning,
    /// A workflow stage completed.
    Completed,
}

/// Sanitized progress event emitted by one operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationEvent {
    /// Job that emitted the event.
    pub job_id: JobId,
    /// Monotonic sequence number scoped to the job, starting at one.
    pub sequence: u64,
    /// Workflow stage that emitted the event.
    pub stage: OperationStage,
    /// Semantic event kind.
    pub kind: OperationEventKind,
    /// Sanitized event-specific fields.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
}

/// Nonblocking event-delivery failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventSinkError {
    /// The bounded sink has no capacity at the moment.
    Full,
    /// The event consumer has been closed.
    Closed,
}

/// Runtime-neutral port for bounded, nonblocking operation events.
///
/// Implementations must return quickly and must never wait for capacity.
/// Operation success cannot depend on event delivery.
pub trait EventSink: Send + Sync {
    /// Attempts to deliver an event without blocking the operation.
    fn try_emit(&self, event: OperationEvent) -> Result<(), EventSinkError>;
}

/// Event sink that intentionally discards every event.
#[derive(Clone, Copy, Debug, Default)]
pub struct DiscardEvents;

impl EventSink for DiscardEvents {
    fn try_emit(&self, _: OperationEvent) -> Result<(), EventSinkError> {
        Ok(())
    }
}

/// Result of a best-effort event emission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventDelivery {
    /// The sink accepted the event.
    Delivered,
    /// The event was dropped because the sink was full.
    DroppedFull,
    /// The event was dropped because the sink was closed.
    DroppedClosed,
}

/// Runtime-neutral context shared by every stage of one operation.
#[derive(Clone)]
pub struct OperationContext {
    /// Stable job identifier.
    pub job_id: JobId,
    /// Optional monotonic deadline.
    pub deadline: Option<Instant>,
    /// Cooperative cancellation token.
    pub cancellation: CancellationToken,
    /// Best-effort operation event sink.
    pub events: Arc<dyn EventSink>,
    sequence: Arc<AtomicU64>,
}

impl OperationContext {
    /// Creates a fresh operation and its presentation-owned cancellation handle.
    pub fn create(
        timeout: Option<Duration>,
        events: Arc<dyn EventSink>,
    ) -> (CancellationHandle, Self) {
        let sequence = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
        let job_id = JobId::new(format!("job-{sequence}"))
            .expect("generated operation job IDs satisfy the domain invariant");
        let (handle, cancellation) = CancellationHandle::new_pair();
        let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));
        (handle, Self::new(job_id, deadline, cancellation, events))
    }

    /// Creates a context for callers that do not need events or cancellation.
    ///
    /// Production frontends should prefer [`OperationContext::create`].
    pub fn detached() -> Self {
        Self::create(None, Arc::new(DiscardEvents)).1
    }

    /// Creates an operation context with an optional monotonic deadline.
    pub fn new(
        job_id: JobId,
        deadline: Option<Instant>,
        cancellation: CancellationToken,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            job_id,
            deadline,
            cancellation,
            events,
            sequence: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Creates an operation context whose deadline is relative to now.
    ///
    /// If adding the duration overflows [`Instant`], the context has no
    /// deadline rather than using a wrapped or already-expired instant.
    pub fn with_timeout(
        job_id: JobId,
        timeout: Duration,
        cancellation: CancellationToken,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self::new(
            job_id,
            Instant::now().checked_add(timeout),
            cancellation,
            events,
        )
    }

    /// Checks cooperative cancellation and deadline expiration at a stage.
    ///
    /// Explicit cancellation wins when both conditions are true.
    pub fn checkpoint(&self, stage: OperationStage) -> SfumatoResult<()> {
        self.cancellation.checkpoint(Some(stage))?;
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(SfumatoError::deadline_exceeded(Some(stage)));
        }
        Ok(())
    }

    /// Returns the remaining deadline duration, or `None` without a deadline.
    ///
    /// Expired deadlines return `Some(Duration::ZERO)`.
    pub fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    /// Emits a best-effort event without blocking or changing operation state.
    pub fn emit(
        &self,
        stage: OperationStage,
        kind: OperationEventKind,
        fields: BTreeMap<String, String>,
    ) -> EventDelivery {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let event = OperationEvent {
            job_id: self.job_id.clone(),
            sequence,
            stage,
            kind,
            fields,
        };
        match self.events.try_emit(event) {
            Ok(()) => EventDelivery::Delivered,
            Err(EventSinkError::Full) => EventDelivery::DroppedFull,
            Err(EventSinkError::Closed) => EventDelivery::DroppedClosed,
        }
    }
}

impl std::fmt::Debug for OperationContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OperationContext")
            .field("job_id", &self.job_id)
            .field("deadline", &self.deadline)
            .field("cancelled", &self.cancellation.is_cancelled())
            .finish_non_exhaustive()
    }
}
