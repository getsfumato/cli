//! `file://` URLs for the paths handed to a browser.
//!
//! The renderers used to build these with `format!("file://{}", path.display())`,
//! which is wrong in two ways that both reach real users:
//!
//! - **Nothing was encoded.** A home directory containing a space — ordinary on
//!   macOS — produced `file:///Users/Alex Fiorenza/…`, and a `#` anywhere in the
//!   path truncated the URL at it. Neither is a Windows-only problem.
//! - **Windows paths came out unusable.** `canonicalize` returns the
//!   extended-length form, so the result was `file://\\?\C:\…`, which Chrome
//!   rejects outright.
//!
//! Built as data rather than through `cfg!` so both shapes are testable from
//! either platform.

use std::path::Path;

/// Characters left alone: RFC 3986 unreserved, plus the separator and the colon a
/// Windows drive letter needs.
fn safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':')
}

/// Turns an absolute path into a `file://` URL.
///
/// `windows` selects the path grammar rather than reading the host, so a Windows
/// path can be converted under test on a unix machine and the other way round.
pub(crate) fn file_url_for(path: &str, windows: bool) -> String {
    let mut rest = path;
    if windows {
        // `canonicalize` hands back the extended-length form. The prefix is an
        // instruction to the Win32 path parser, not part of the path, and a URL
        // must not carry it. `\\?\UNC\server\share` is the network form, which
        // becomes an authority rather than a local path.
        if let Some(unc) = rest.strip_prefix(r"\\?\UNC\") {
            let encoded = encode(&unc.replace('\\', "/"));
            return format!("file://{encoded}");
        }
        rest = rest.strip_prefix(r"\\?\").unwrap_or(rest);
    }

    let slashed = if windows {
        rest.replace('\\', "/")
    } else {
        rest.to_owned()
    };
    let encoded = encode(&slashed);

    // Three slashes: an empty authority, then a path that must itself start with
    // one. A Windows path starts at a drive letter, so it needs the slash added.
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if safe(byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

/// Turns a path into a `file://` URL using this platform's grammar.
pub(crate) fn file_url(path: &Path) -> String {
    file_url_for(&path.to_string_lossy(), cfg!(windows))
}

#[cfg(test)]
#[path = "../tests/unit/file_urls.rs"]
mod tests;
