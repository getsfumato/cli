use super::*;

#[test]
fn discovers_supported_sources_in_deterministic_order() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("b.txt"), "B").unwrap();
    std::fs::write(temp.path().join("a.md"), "A").unwrap();
    std::fs::write(temp.path().join("ignored.bin"), "ignored").unwrap();

    let documents = FilesystemSourceReader
        .collect(&[temp.path().to_path_buf()])
        .unwrap();

    assert_eq!(documents.len(), 2);
    assert!(documents[0].path.ends_with("a.md"));
    assert!(documents[1].path.ends_with("b.txt"));
}

#[test]
fn loads_optional_project_instructions() {
    let temp = tempfile::tempdir().unwrap();
    assert!(
        FilesystemSourceReader
            .project_instructions(temp.path())
            .unwrap()
            .is_none()
    );

    let path = temp.path().join(PROJECT_INSTRUCTIONS_FILE);
    std::fs::write(&path, "# Guidance\n\nTeach visually.\n").unwrap();
    let instructions = FilesystemSourceReader
        .project_instructions(temp.path())
        .unwrap()
        .unwrap();
    assert_eq!(instructions.path, path);
    assert_eq!(instructions.content, "# Guidance\n\nTeach visually.");
}

#[test]
fn rejects_oversized_project_instructions() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join(PROJECT_INSTRUCTIONS_FILE),
        vec![b'x'; MAX_PROJECT_INSTRUCTIONS_BYTES as usize + 1],
    )
    .unwrap();
    assert!(
        FilesystemSourceReader
            .project_instructions(temp.path())
            .unwrap_err()
            .to_string()
            .contains("maximum")
    );
}
