use super::*;

use std::{convert::Infallible, sync::Arc, time::Instant};

use sfumato_core::{
    errors::{ErrorClass, ErrorCode},
    operation::DiscardEvents,
};

#[tokio::test]
async fn pending_adapter_work_observes_cancellation() {
    let (handle, operation) = OperationContext::create(None, Arc::new(DiscardEvents));
    let task = tokio::spawn(async move {
        await_operation(
            &operation,
            OperationStage::Draft,
            std::future::pending::<Result<(), Infallible>>(),
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(30)).await;
    handle.cancel();
    let error = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("cancelled adapter work should finish promptly")
        .unwrap()
        .unwrap_err();
    let error = error.downcast_ref::<SfumatoError>().unwrap();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.class, ErrorClass::Cancelled);
    assert_eq!(error.stage, Some(OperationStage::Draft));
}

#[tokio::test]
async fn pending_adapter_work_observes_deadline() {
    let (_, operation) =
        OperationContext::create(Some(Duration::from_millis(30)), Arc::new(DiscardEvents));
    let started = Instant::now();
    let error = await_operation(
        &operation,
        OperationStage::Review,
        std::future::pending::<Result<(), Infallible>>(),
    )
    .await
    .unwrap_err();

    assert!(started.elapsed() < Duration::from_secs(1));
    let error = error.downcast_ref::<SfumatoError>().unwrap();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert_eq!(error.stage, Some(OperationStage::Review));
    assert_eq!(error.details["reason"], "deadline_exceeded");
}

#[cfg(unix)]
#[tokio::test]
async fn child_process_is_bounded_by_operation_cancellation() {
    let (handle, operation) = OperationContext::create(None, Arc::new(DiscardEvents));
    let task = tokio::spawn(async move {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 10"]);
        run_command(&mut command, &operation, OperationStage::Render).await
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    handle.cancel();
    let error = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("cancelled child process should finish promptly")
        .unwrap()
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<SfumatoError>().unwrap().stage,
        Some(OperationStage::Render)
    );
}
