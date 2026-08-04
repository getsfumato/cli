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

#[test]
fn a_partly_binary_source_is_refused_instead_of_losing_its_tail() {
    // The pop loop discarded everything after the first invalid byte and handed the
    // surviving prefix to the model as if it were the whole file: 96% of a 52 KB
    // file in the reported case, with no marker and no warning.
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("half.json");
    let mut bytes = b"{\"note\":\"IMPORTANT REAL CONTENT\"}\n".repeat(70);
    bytes.extend(std::iter::repeat_n(0xF8_u8, 50_000));
    std::fs::write(&path, &bytes).unwrap();

    let error = FilesystemSourceReader
        .collect(std::slice::from_ref(&path))
        .expect_err("a mostly-binary source is refused");

    assert!(
        error.message.contains("not valid UTF-8"),
        "{}",
        error.message
    );
    assert!(error.message.contains("50000"), "{}", error.message);
    assert!(error.message.contains("half.json"), "{}", error.message);
}

#[test]
fn a_fully_binary_source_is_refused_rather_than_included_empty() {
    // The loop popped everything, `from_utf8` succeeded on the empty buffer, and the
    // model got a source header with nothing under it.
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("blob.txt");
    std::fs::write(&path, vec![0xFF_u8; 4_096]).unwrap();

    assert!(FilesystemSourceReader.collect(&[path]).is_err());
}

#[test]
fn valid_utf8_sources_are_still_read_whole() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("notes.md");
    let text = "# Guía\n\nAcentos: ñ á é í ó ú — y CJK: 日本語\n";
    std::fs::write(&path, text).unwrap();

    let documents = FilesystemSourceReader.collect(&[path]).unwrap();

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].content, text);
}

#[test]
fn a_multibyte_character_cut_by_the_byte_cap_is_still_repaired_silently() {
    // The trim exists for this: the cap can slice one character in half, and
    // discarding those few bytes is the intended repair, not a loss to report.
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("big.md");
    // Fill past the per-file cap with a three-byte character so the cap lands
    // mid-character.
    let text = "日".repeat(400_000);
    std::fs::write(&path, &text).unwrap();

    let documents = FilesystemSourceReader
        .collect(&[path])
        .expect("a cut character is repaired, not refused");

    assert_eq!(documents.len(), 1);
    assert!(
        documents[0]
            .content
            .contains("truncated by sfumato preflight"),
        "the size cap still marks its own truncation"
    );
}
