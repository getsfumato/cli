use super::*;

fn report(payload: &str) -> String {
    let encoded = payload
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect::<String>();
    format!("<html data-sfumato-format=\"{encoded}\"><body></body></html>")
}

#[test]
fn parses_a_report_with_one_defect() {
    let dom = report(
        r#"{"pages":4,"issues":[{"page":2,"section":1,"heading":"Primero","kind":"overflows_text_column","overflow_px":34,"element":"table"}]}"#,
    );

    let issues = parse_format_report(&dom).expect("the report parses");

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].page, 2);
    assert_eq!(issues[0].section, 1);
    assert_eq!(issues[0].heading, "Primero");
    assert_eq!(issues[0].overflow_px, 34);
    assert_eq!(issues[0].element, "table");
    assert_eq!(
        issues[0].kind,
        sfumato_core::generation::DocumentFormatIssueKind::OverflowsTextColumn
    );
}

#[test]
fn a_clean_document_reports_no_defects() {
    let dom = report(r#"{"pages":3,"issues":[]}"#);

    assert!(
        parse_format_report(&dom)
            .expect("the report parses")
            .is_empty()
    );
}

#[test]
fn a_report_with_no_pages_is_an_error() {
    let dom = report(r#"{"pages":0,"issues":[]}"#);

    let error = parse_format_report(&dom).expect_err("a document with no pages is an error");

    assert!(format!("{error}").contains("no page boxes"), "{error}");
}

#[test]
fn a_missing_report_is_an_error() {
    let error = parse_format_report("<html><body></body></html>")
        .expect_err("a missing report is an error");

    assert!(format!("{error}").contains("did not return"), "{error}");
}

#[test]
fn reads_the_page_count_the_cli_reported() {
    // The CLI writes PDFs whose page objects sit inside compressed object
    // streams, so scanning the bytes cannot count them; its own report can.
    assert_eq!(
        reported_pages("\u{2714} Rendering 6 pages took 41.1 milliseconds."),
        Some(6)
    );
    assert_eq!(
        reported_pages("- Rendering: Page 1\nRendering 12 pages"),
        Some(12)
    );
}

#[test]
fn output_without_a_page_report_is_not_guessed_at() {
    assert_eq!(
        reported_pages("\u{2714} Loaded\n- Generating\n\u{2714} Saved"),
        None
    );
    assert_eq!(reported_pages("Rendering many pages"), None);
}

#[test]
fn a_missing_browser_is_reported_as_unavailable_not_permanent() {
    // Same string agreement as the slide and page renderers.
    let error = render_result::<()>(
        Err(anyhow::anyhow!(crate::browser::not_found(
            "to measure the document"
        ))),
        OperationStage::Render,
    )
    .expect_err("the error is propagated");
    assert_eq!(error.class, ErrorClass::Unavailable);
}
