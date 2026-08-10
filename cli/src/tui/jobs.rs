//! Job identity, cancellation, and the handles the views can stop.
//!
//! Separated from the rest of the state because it is the part that owns
//! something outside the process: a running task and the token that stops it.
//! Everything else in the model is data the views read.

use super::*;
use std::sync::Arc;

use sfumato_core::operation::{CancellationHandle, EventSink, OperationContext};

/// Owns UI job identity and the matching core cancellation handle.
pub(super) struct OperationLifecycle {
    next_job_id: u64,
    active_job_id: Option<u64>,
    cancellation: Option<CancellationHandle>,
}

impl Default for OperationLifecycle {
    fn default() -> Self {
        Self {
            next_job_id: 1,
            active_job_id: None,
            cancellation: None,
        }
    }
}

/// A running native connector read that the view can stop.
pub(super) struct ConnectorQuery {
    cancellation: CancellationHandle,
    task: JoinHandle<()>,
}

impl ConnectorQuery {
    pub(super) fn new(cancellation: CancellationHandle, task: JoinHandle<()>) -> Self {
        Self { cancellation, task }
    }

    /// Signals cancellation and drops the task without waiting on it.
    ///
    /// `Esc` must return control immediately; the task observes the token at its
    /// next checkpoint and its result is discarded because the view has moved on.
    pub(super) fn cancel(self) {
        self.cancellation.cancel();
        self.task.abort();
    }

    /// Signals cancellation and awaits the task, for shutdown.
    pub(super) async fn cancel_and_join(self) {
        self.cancellation.cancel();
        self.task.abort();
        let _ = self.task.await;
    }
}

impl OperationLifecycle {
    pub(super) fn next_job_id(&self) -> u64 {
        self.next_job_id
    }

    pub(super) fn begin(&mut self, events: Arc<dyn EventSink>) -> (u64, OperationContext) {
        self.cancel();
        let job_id = self.next_job_id;
        self.next_job_id = self.next_job_id.wrapping_add(1).max(1);
        let (cancellation, operation) = OperationContext::create(None, events);
        self.active_job_id = Some(job_id);
        self.cancellation = Some(cancellation);
        (job_id, operation)
    }

    pub(super) fn is_active(&self, job_id: u64) -> bool {
        self.active_job_id == Some(job_id)
    }

    pub(super) fn finish(&mut self, job_id: u64) -> bool {
        if !self.is_active(job_id) {
            return false;
        }
        self.active_job_id = None;
        self.cancellation = None;
        true
    }

    pub(super) fn cancel(&self) {
        if let Some(cancellation) = &self.cancellation {
            cancellation.cancel();
        }
    }
}
