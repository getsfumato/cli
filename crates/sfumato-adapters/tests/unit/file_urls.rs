//! `file://` construction, for both path grammars, from either platform.

use super::*;

fn unix(path: &str) -> String {
    file_url_for(path, false)
}

fn windows(path: &str) -> String {
    file_url_for(path, true)
}

#[test]
fn a_plain_unix_path_keeps_its_leading_slash() {
    assert_eq!(unix("/tmp/deck.html"), "file:///tmp/deck.html");
}

#[test]
fn encodes_a_space() {
    // The case that reaches macOS users: a home directory with a space in it.
    assert_eq!(
        unix("/Users/Alex Fiorenza/deck.html"),
        "file:///Users/Alex%20Fiorenza/deck.html"
    );
}

#[test]
fn encodes_a_hash_rather_than_truncating_at_it() {
    // Unencoded, everything after the `#` becomes a fragment and the browser asks
    // for a file that does not exist.
    assert_eq!(unix("/tmp/c#1/deck.html"), "file:///tmp/c%231/deck.html");
}

#[test]
fn encodes_a_question_mark_rather_than_starting_a_query() {
    assert_eq!(unix("/tmp/what?/a.html"), "file:///tmp/what%3F/a.html");
}

#[test]
fn encodes_a_percent_so_that_encoding_is_not_ambiguous() {
    // Without this, a literal `%20` in a filename and an encoded space would be
    // indistinguishable.
    assert_eq!(unix("/tmp/100%/a.html"), "file:///tmp/100%25/a.html");
}

#[test]
fn encodes_non_ascii_as_utf8_bytes() {
    assert_eq!(unix("/tmp/física.html"), "file:///tmp/f%C3%ADsica.html");
}

#[test]
fn leaves_unreserved_characters_alone() {
    assert_eq!(
        unix("/tmp/a-b_c.d~e/f.html"),
        "file:///tmp/a-b_c.d~e/f.html"
    );
}

#[test]
fn a_windows_path_gains_the_third_slash_and_forward_separators() {
    assert_eq!(
        windows(r"C:\Users\alex\deck.html"),
        "file:///C:/Users/alex/deck.html"
    );
}

#[test]
fn strips_the_extended_length_prefix_canonicalize_returns() {
    // This is the whole Windows bug: `canonicalize` yields the `\\?\` form, and
    // `file://\\?\C:\…` is rejected by the browser.
    assert_eq!(
        windows(r"\\?\C:\Users\alex\deck.html"),
        "file:///C:/Users/alex/deck.html"
    );
}

#[test]
fn a_unc_path_becomes_an_authority() {
    // `\\?\UNC\server\share\x` is a network path; the server belongs in the
    // authority, not after a third slash.
    assert_eq!(
        windows(r"\\?\UNC\server\share\deck.html"),
        "file://server/share/deck.html"
    );
}

#[test]
fn encodes_a_space_in_a_windows_path_too() {
    assert_eq!(
        windows(r"\\?\C:\Program Files\deck.html"),
        "file:///C:/Program%20Files/deck.html"
    );
}

#[test]
fn keeps_the_drive_colon_unencoded() {
    // Encoding it as %3A works in some browsers and not others; leaving it is what
    // every other tool emits.
    assert!(windows(r"C:\a.html").starts_with("file:///C:/"));
}

#[test]
fn the_platform_helper_uses_this_platforms_grammar() {
    let url = file_url(std::path::Path::new("/tmp/deck.html"));
    if cfg!(windows) {
        // A unix-shaped path on Windows still round-trips to something well-formed.
        assert!(url.starts_with("file:///"));
    } else {
        assert_eq!(url, "file:///tmp/deck.html");
    }
}
