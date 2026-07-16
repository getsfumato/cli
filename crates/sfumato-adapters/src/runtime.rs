//! Async runtime helpers that enforce core operation cancellation and deadlines.

use std::{
    future::Future,
    process::{Output, Stdio},
    time::Duration,
};

use anyhow::Result;
use sfumato_core::{
    errors::{OperationStage, SfumatoError},
    operation::{CancellationToken, OperationContext},
};
use tokio::process::Command;

/// Awaits adapter work while observing the operation token and deadline.
pub(crate) async fn await_operation<F, T, E>(
    operation: &OperationContext,
    stage: OperationStage,
    future: F,
) -> Result<T>
where
    F: Future<Output = Result<T, E>>,
    E: Into<anyhow::Error>,
{
    operation.checkpoint(stage)?;
    tokio::pin!(future);
    let cancellation = wait_for_cancellation(operation.cancellation.clone());
    let deadline = wait_for_deadline(operation.remaining());
    tokio::pin!(cancellation);
    tokio::pin!(deadline);

    tokio::select! {
        result = &mut future => result.map_err(Into::into),
        () = &mut cancellation => Err(SfumatoError::cancelled(Some(stage)).into()),
        () = &mut deadline => Err(SfumatoError::deadline_exceeded(Some(stage)).into()),
    }
}

/// Runs a child process whose lifetime is bounded by one operation.
pub(crate) async fn run_command(
    command: &mut Command,
    operation: &OperationContext,
    stage: OperationStage,
) -> Result<Output> {
    operation.checkpoint(stage)?;
    command
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn()?;
    await_operation(operation, stage, child.wait_with_output()).await
}

async fn wait_for_cancellation(token: CancellationToken) {
    while !token.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for_deadline(remaining: Option<Duration>) {
    match remaining {
        Some(remaining) => tokio::time::sleep(remaining).await,
        None => std::future::pending::<()>().await,
    }
}

#[cfg(test)]
#[path = "../tests/unit/runtime.rs"]
mod tests;
