use super::*;

use sfumato_core::operation::DiscardEvents;

#[test]
fn lifecycle_rejects_stale_job_completion() {
    let mut lifecycle = OperationLifecycle::default();
    let (first, _) = lifecycle.begin(Arc::new(DiscardEvents));
    let (second, _) = lifecycle.begin(Arc::new(DiscardEvents));

    assert!(!lifecycle.finish(first));
    assert!(lifecycle.is_active(second));
    assert!(lifecycle.finish(second));
}

#[test]
fn starting_a_new_job_cancels_the_previous_context() {
    let mut lifecycle = OperationLifecycle::default();
    let (_, first) = lifecycle.begin(Arc::new(DiscardEvents));
    let _ = lifecycle.begin(Arc::new(DiscardEvents));

    assert!(first.cancellation.is_cancelled());
}
