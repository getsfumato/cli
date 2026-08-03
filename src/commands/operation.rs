//! Operation contexts that observe Ctrl-C and the process-wide `--timeout`.
//!
//! Every command used to build `OperationContext::detached()`, which carries no
//! deadline and a cancellation token nobody ever signals. That left two ways to
//! wait forever — a provider request or generated Python with no deadline, and a
//! Ctrl-C that killed the process without running any `Drop`, so staged files
//! were abandoned rather than cleaned up.
//!
//! Cancelling cooperatively is what makes that cleanup happen: the token turns
//! into an error that unwinds through the workflow, the artifact transaction is
//! dropped on the way out, and its staging directory goes with it.

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use sfumato_core::operation::{CancellationHandle, DiscardEvents, OperationContext};

/// Process-wide deadline parsed from `--timeout`.
///
/// A global rather than a parameter threaded through every command: the flag is
/// parsed once, before any command runs, and is never written again. Passing it
/// by hand would mean changing the signature of every `RunnableCommand`
/// implementation to carry a value none of them make a decision about.
static TIMEOUT: OnceLock<Option<Duration>> = OnceLock::new();

/// Records the deadline for this process. Called once, from `main`.
pub fn set_timeout(seconds: Option<u64>) {
    let timeout = seconds.map(Duration::from_secs);
    let _ = TIMEOUT.set(timeout);
}

/// The deadline every operation in this process runs under.
fn timeout() -> Option<Duration> {
    TIMEOUT.get().copied().flatten()
}

/// Builds an operation bounded by `--timeout` and cancellable with Ctrl-C.
///
/// Prefer this to `OperationContext::detached()` in any command that reaches a
/// provider, a renderer, or a subprocess.
pub fn interruptible() -> OperationContext {
    interruptible_with(timeout())
}

/// The body of [`interruptible`], with the deadline passed rather than read.
///
/// Split out so it can be exercised at several deadlines in one process:
/// [`TIMEOUT`] is written once and a test cannot set it twice.
fn interruptible_with(timeout: Option<Duration>) -> OperationContext {
    let (handle, operation) = OperationContext::create(timeout, Arc::new(DiscardEvents));
    tokio::spawn(watch_for_interrupt(handle));
    operation
}

/// Turns the first Ctrl-C into a cancellation request and the second into an exit.
///
/// Registering a handler means the process no longer dies on Ctrl-C, so the first
/// one has to lead somewhere the user can see. It asks the operation to stop,
/// which lets the workflow unwind and clean up its staging directory. A second
/// Ctrl-C is a user who does not want to wait for that, and is honoured
/// immediately — at the cost of the cleanup, which is said out loud.
async fn watch_for_interrupt(handle: CancellationHandle) {
    if tokio::signal::ctrl_c().await.is_err() {
        return;
    }
    if handle.cancel() {
        eprintln!(
            "\nInterrupted. Stopping at the next checkpoint and removing staged files. Press Ctrl-C again to quit now."
        );
    }
    if tokio::signal::ctrl_c().await.is_ok() {
        eprintln!("Quitting now; staged files are left behind.");
        // 130 is the conventional shell status for a process ended by SIGINT.
        std::process::exit(130);
    }
}

#[cfg(test)]
#[path = "../../tests/unit/commands_operation.rs"]
mod tests;
