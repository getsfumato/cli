//! State transitions produced by asynchronous resource-operation messages.

use super::*;

pub(super) fn reduce_message(app: &mut App, message: UiMessage) {
    match message {
        UiMessage::GenerationEvent { job_id, event } => {
            if !app.jobs.is_active(job_id) {
                return;
            }
            if let TextGenerationEvent::StageStarted { stage, .. } = &event {
                app.current_stage = Some(*stage);
            }
            let activity = Activity::from_event(&event);
            let image_path = activity.image_path.clone();
            app.activities.push(activity);
            app.activity_index = app.activities.len().saturating_sub(1);
            if let Some(path) = image_path {
                app.load_image(&path);
            }
        }
        UiMessage::OperationEvent { job_id, event } => {
            if !app.jobs.is_active(job_id) {
                return;
            }
            if let Some(activity) = Activity::from_operation_event(&event) {
                app.activities.push(activity);
                app.activity_index = app.activities.len().saturating_sub(1);
            }
        }
        UiMessage::ResourceFinished { job_id, result } => {
            if !app.jobs.finish(job_id) {
                return;
            }
            app.active_task = None;
            match *result {
                Ok(result) => {
                    for warning in result.warnings() {
                        app.activities.push(Activity {
                            kind: ActivityKind::Warning,
                            title: "Resource warning".to_string(),
                            detail: warning.clone(),
                            image_path: None,
                        });
                    }
                    app.status = Some((result.completion_message().to_string(), false));
                    app.result = Some(result);
                }
                Err(error) => {
                    app.generation_failed = true;
                    app.activities.push(Activity {
                        kind: ActivityKind::Warning,
                        title: "Resource operation failed".to_string(),
                        detail: error.clone(),
                        image_path: None,
                    });
                    app.status = Some((error, true));
                }
            }
            app.activity_index = app.activities.len().saturating_sub(1);
            app.transition(Screen::Complete);
        }
        UiMessage::ResourceCancelled { job_id } => {
            if !app.jobs.finish(job_id) {
                return;
            }
            app.active_task = None;
            app.status = Some(("Operation cancelled".to_string(), false));
            app.activities.push(Activity {
                kind: ActivityKind::Warning,
                title: "Operation cancelled".to_string(),
                detail: "No staged artifacts were committed.".to_string(),
                image_path: None,
            });
            app.activity_index = app.activities.len().saturating_sub(1);
            app.transition(Screen::Complete);
        }
    }
}
