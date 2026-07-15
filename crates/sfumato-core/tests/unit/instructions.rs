use super::*;

#[test]
fn loads_project_root_instructions() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(PROJECT_INSTRUCTIONS_FILE);
    fs::write(&path, "# Guidance\n\nTeach visually.\n").unwrap();

    let instructions = ProjectInstructions::load(temp.path()).unwrap().unwrap();

    assert_eq!(instructions.path, path);
    assert_eq!(instructions.content, "# Guidance\n\nTeach visually.");
    assert!(instructions.prompt_section().contains("Teach visually."));
}

#[test]
fn missing_project_instructions_are_optional() {
    let temp = tempfile::tempdir().unwrap();

    assert!(ProjectInstructions::load(temp.path()).unwrap().is_none());
}

#[test]
fn rejects_oversized_project_instructions() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(
        temp.path().join(PROJECT_INSTRUCTIONS_FILE),
        vec![b'x'; MAX_PROJECT_INSTRUCTIONS_BYTES as usize + 1],
    )
    .unwrap();

    let error = ProjectInstructions::load(temp.path()).unwrap_err();

    assert!(error.to_string().contains("maximum"));
}
