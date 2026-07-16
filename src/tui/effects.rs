//! Asynchronous effects that bridge core operations into TUI messages.

use super::*;

pub(super) fn operation_event_sink(job_id: u64, sender: Sender<UiMessage>) -> Arc<dyn EventSink> {
    Arc::new(UiOperationEventSink { job_id, sender })
}

pub(super) fn generation_event_sink(
    job_id: u64,
    sender: Sender<UiMessage>,
) -> Arc<dyn Fn(TextGenerationEvent) + Send + Sync> {
    Arc::new(move |event| {
        let _ = sender.try_send(UiMessage::GenerationEvent { job_id, event });
    })
}

pub(super) fn spawn_generation(
    job_id: u64,
    application: Arc<SfumatoApplication>,
    args: SlidesArgs,
    sink: Arc<dyn Fn(TextGenerationEvent) + Send + Sync>,
    operation: OperationContext,
    sender: Sender<UiMessage>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        match execute_slides(&application, args, Some(sink), operation).await {
            Err(error) if is_cancelled_error(&error) => {
                let _ = sender.send(UiMessage::ResourceCancelled { job_id }).await;
            }
            result => {
                let result = result
                    .map(ResourceResult::Generated)
                    .map_err(|error| format!("{error:#}"));
                let _ = sender
                    .send(UiMessage::ResourceFinished {
                        job_id,
                        result: Box::new(result),
                    })
                    .await;
            }
        }
    })
}

pub(super) fn spawn_edit(
    job_id: u64,
    application: Arc<SfumatoApplication>,
    args: EditSlidesArgs,
    sink: Arc<dyn Fn(TextGenerationEvent) + Send + Sync>,
    operation: OperationContext,
    sender: Sender<UiMessage>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        match execute_edit_slides(&application, args, Some(sink), operation).await {
            Err(error) if is_cancelled_error(&error) => {
                let _ = sender.send(UiMessage::ResourceCancelled { job_id }).await;
            }
            result => {
                let result = result
                    .map(ResourceResult::Edited)
                    .map_err(|error| format!("{error:#}"));
                let _ = sender
                    .send(UiMessage::ResourceFinished {
                        job_id,
                        result: Box::new(result),
                    })
                    .await;
            }
        }
    })
}

struct UiOperationEventSink {
    job_id: u64,
    sender: Sender<UiMessage>,
}

impl EventSink for UiOperationEventSink {
    fn try_emit(&self, event: OperationEvent) -> std::result::Result<(), EventSinkError> {
        self.sender
            .try_send(UiMessage::OperationEvent {
                job_id: self.job_id,
                event,
            })
            .map_err(|error| match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => EventSinkError::Full,
                tokio::sync::mpsc::error::TrySendError::Closed(_) => EventSinkError::Closed,
            })
    }
}

fn is_cancelled_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<SfumatoError>()
        .is_some_and(|error| error.class == ErrorClass::Cancelled)
}
