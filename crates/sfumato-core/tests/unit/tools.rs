use super::*;
use crate::providers::ToolExecutionRequest;

#[test]
fn lists_and_reads_inside_allowed_roots() {
    let temp = tempfile::tempdir().unwrap();
    let note = temp.path().join("note.md");
    fs::write(&note, "# Note").unwrap();
    let executor = FilesystemToolExecutor::new(vec![temp.path().to_path_buf()]).unwrap();

    let listing = executor
        .execute(ToolExecutionRequest {
            name: "sfumato_list_directory".to_string(),
            arguments: json!({ "path": temp.path() }),
        })
        .unwrap();
    assert!(listing.contains("note.md"));

    let content = executor
        .execute(ToolExecutionRequest {
            name: "sfumato_read_file".to_string(),
            arguments: json!({ "path": note }),
        })
        .unwrap();
    assert!(content.contains("# Note"));
}

#[test]
fn rejects_paths_outside_allowed_roots() {
    let allowed = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.md");
    fs::write(&secret, "nope").unwrap();
    let executor = FilesystemToolExecutor::new(vec![allowed.path().to_path_buf()]).unwrap();

    assert!(
        executor
            .execute(ToolExecutionRequest {
                name: "sfumato_read_file".to_string(),
                arguments: json!({ "path": secret }),
            })
            .is_err()
    );
}

#[test]
fn tool_arguments_accept_json_strings() {
    let temp = tempfile::tempdir().unwrap();
    let note = temp.path().join("note.md");
    fs::write(&note, "# Note").unwrap();
    let executor = FilesystemToolExecutor::new(vec![temp.path().to_path_buf()]).unwrap();

    let content = executor
        .execute(ToolExecutionRequest {
            name: "sfumato_read_file".to_string(),
            arguments: Value::String(format!(r#"{{"path":"{}"}}"#, note.display())),
        })
        .unwrap();
    assert!(content.contains("# Note"));
}
