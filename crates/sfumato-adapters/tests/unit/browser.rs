//! Browser discovery order, asserted for every operating system.
//!
//! These run the same on any host: the filesystem and the environment arrive
//! through [`Host`], so a Linux or Windows layout can be described from a Mac.
//! The previous implementation probed the real filesystem directly, which is why
//! its Linux behaviour — returning `None` unconditionally — was never noticed.

use std::{collections::BTreeMap, ffi::OsString};

use super::*;

/// A machine described rather than inspected.
#[derive(Default)]
struct FakeHost {
    variables: BTreeMap<String, OsString>,
    files: Vec<String>,
}

impl FakeHost {
    fn with_variable(mut self, key: &str, value: &str) -> Self {
        self.variables.insert(key.to_owned(), OsString::from(value));
        self
    }

    fn with_files(mut self, paths: &[&str]) -> Self {
        self.files
            .extend(paths.iter().map(|path| (*path).to_owned()));
        self
    }
}

impl Host for FakeHost {
    fn variable(&self, key: &str) -> Option<OsString> {
        self.variables.get(key).cloned()
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.iter().any(|known| Path::new(known) == path)
    }
}

#[test]
fn finds_nothing_on_a_machine_with_no_browser() {
    assert_eq!(detect(&FakeHost::default(), &LINUX), None);
    assert_eq!(detect(&FakeHost::default(), &MACOS), None);
    assert_eq!(detect(&FakeHost::default(), &WINDOWS), None);
}

#[test]
fn finds_a_browser_on_path_on_linux() {
    // The case the old implementation could never satisfy.
    let host = FakeHost::default()
        .with_variable("PATH", "/usr/local/bin:/usr/bin:/bin")
        .with_files(&["/usr/bin/chromium"]);

    assert_eq!(
        detect(&host, &LINUX),
        Some(PathBuf::from("/usr/bin/chromium"))
    );
}

#[test]
fn finds_a_macos_bundle_that_is_not_on_path() {
    // A bundle is never on PATH, so `well_known` is the only step that can find
    // it. This is what the scan was for, and why it stays as the last resort.
    let host = FakeHost::default()
        .with_variable("PATH", "/usr/bin:/bin")
        .with_files(&["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"]);

    assert_eq!(
        detect(&host, &MACOS),
        Some(PathBuf::from(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
        ))
    );
}

#[test]
fn appends_the_executable_suffix_on_windows() {
    // `chrome` on PATH is `chrome.exe` on disk, and the separator is `;`.
    let host = FakeHost::default()
        .with_variable("PATH", r"C:\Windows;C:\Tools")
        .with_files(&[r"C:\Tools\chrome.exe"]);

    assert_eq!(
        detect(&host, &WINDOWS),
        Some(PathBuf::from(r"C:\Tools\chrome.exe"))
    );
}

#[test]
fn falls_back_to_a_well_known_location_on_windows() {
    let host = FakeHost::default()
        .with_variable("PATH", r"C:\Windows")
        .with_files(&[r"C:\Program Files\Google\Chrome\Application\chrome.exe"]);

    assert_eq!(
        detect(&host, &WINDOWS),
        Some(PathBuf::from(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe"
        ))
    );
}

#[test]
fn prefers_an_environment_variable_over_path_and_well_known() {
    let host = FakeHost::default()
        .with_variable("SFUMATO_BROWSER", "/opt/custom/chrome")
        .with_variable("PATH", "/usr/bin")
        .with_files(&[
            "/opt/custom/chrome",
            "/usr/bin/chromium",
            "/usr/bin/google-chrome",
        ]);

    assert_eq!(
        detect(&host, &LINUX),
        Some(PathBuf::from("/opt/custom/chrome"))
    );
}

#[test]
fn honours_the_variables_the_underlying_tools_already_read() {
    // A machine set up for puppeteer or mmdc should not have to be told twice.
    for key in ["PUPPETEER_EXECUTABLE_PATH", "CHROME_PATH"] {
        let host = FakeHost::default()
            .with_variable(key, "/opt/pinned/chrome")
            .with_files(&["/opt/pinned/chrome"]);

        assert_eq!(
            detect(&host, &LINUX),
            Some(PathBuf::from("/opt/pinned/chrome")),
            "{key} was ignored"
        );
    }
}

#[test]
fn prefers_sfumatos_own_variable_over_the_borrowed_ones() {
    let host = FakeHost::default()
        .with_variable("SFUMATO_BROWSER", "/opt/ours/chrome")
        .with_variable("PUPPETEER_EXECUTABLE_PATH", "/opt/theirs/chrome")
        .with_files(&["/opt/ours/chrome", "/opt/theirs/chrome"]);

    assert_eq!(
        detect(&host, &LINUX),
        Some(PathBuf::from("/opt/ours/chrome"))
    );
}

#[test]
fn skips_an_environment_variable_that_points_at_nothing() {
    // Falling through is right: a stale variable in a shell profile should not
    // make a machine with a working browser look like one without.
    let host = FakeHost::default()
        .with_variable("SFUMATO_BROWSER", "/opt/removed/chrome")
        .with_variable("PATH", "/usr/bin")
        .with_files(&["/usr/bin/chromium"]);

    assert_eq!(
        detect(&host, &LINUX),
        Some(PathBuf::from("/usr/bin/chromium"))
    );
}

#[test]
fn treats_an_empty_variable_as_unset() {
    let host = FakeHost::default()
        .with_variable("SFUMATO_BROWSER", "")
        .with_variable("PATH", "/usr/bin")
        .with_files(&["/usr/bin/chromium"]);

    assert_eq!(
        detect(&host, &LINUX),
        Some(PathBuf::from("/usr/bin/chromium"))
    );
}

#[test]
fn follows_the_preference_order_of_the_names_not_the_order_of_path() {
    // Within one directory, `google-chrome` outranks `chromium` because the table
    // says so.
    let host = FakeHost::default()
        .with_variable("PATH", "/usr/bin")
        .with_files(&["/usr/bin/chromium", "/usr/bin/google-chrome"]);

    assert_eq!(
        detect(&host, &LINUX),
        Some(PathBuf::from("/usr/bin/google-chrome"))
    );
}

#[test]
fn searches_path_directories_in_order() {
    // An earlier directory wins even with a less preferred name, which is what
    // makes prepending a directory to PATH work as an override.
    let host = FakeHost::default()
        .with_variable("PATH", "/opt/bin:/usr/bin")
        .with_files(&["/opt/bin/chromium", "/usr/bin/google-chrome"]);

    assert_eq!(
        detect(&host, &LINUX),
        Some(PathBuf::from("/opt/bin/chromium"))
    );
}

#[test]
fn ignores_empty_path_entries() {
    // A trailing or doubled separator would otherwise probe the process's own
    // working directory, which is not a place a browser should be found.
    let host = FakeHost::default()
        .with_variable("PATH", ":/usr/bin::")
        .with_files(&["chromium", "/usr/bin/chromium"]);

    assert_eq!(
        detect(&host, &LINUX),
        Some(PathBuf::from("/usr/bin/chromium"))
    );
}

#[test]
fn prefers_path_over_a_well_known_location() {
    let host = FakeHost::default()
        .with_variable("PATH", "/opt/bin")
        .with_files(&["/opt/bin/chromium", "/usr/bin/google-chrome"]);

    assert_eq!(
        detect(&host, &LINUX),
        Some(PathBuf::from("/opt/bin/chromium"))
    );
}

#[test]
fn a_configured_path_that_exists_is_used_verbatim() {
    let temporary = tempfile::tempdir().unwrap();
    let browser = temporary.path().join("chrome");
    std::fs::write(&browser, b"#!/bin/sh\n").unwrap();

    assert_eq!(resolve(Some(&browser)).unwrap(), Some(browser));
}

#[test]
fn a_configured_path_that_does_not_exist_is_an_error() {
    // Not a fall through to detection: rendering with a browser the user did not
    // choose is worse than saying the configured one is gone.
    let temporary = tempfile::tempdir().unwrap();
    let missing = temporary.path().join("absent");

    let error = resolve(Some(&missing)).expect_err("a missing configured browser is refused");

    let message = format!("{error:#}");
    assert!(message.contains("does not exist"), "{message}");
    assert!(message.contains("absent"), "{message}");
}

#[test]
fn the_not_found_message_keeps_the_prefix_its_callers_match_on() {
    // marp.rs, pages.rs and renderers/document.rs all classify a missing browser
    // as `ErrorClass::Unavailable` by matching this prefix. If it ever stops
    // being a prefix of this message, a missing browser silently becomes a
    // permanent error and the operation stops being retried.
    let message = not_found("for page inspection");

    assert!(
        message.starts_with("Could not find Chrome"),
        "the classified prefix is gone: {message}"
    );
    assert!(message.contains("for page inspection"), "{message}");
    // The remedy is the part a user acts on, so it is worth asserting too.
    assert!(message.contains("marp.browser_path"), "{message}");
    assert!(message.contains("SFUMATO_BROWSER"), "{message}");
}

#[test]
fn the_host_table_is_the_one_for_this_build() {
    // Cheap, but it is the only thing tying the tested tables to what ships.
    let candidates = host_candidates();
    if cfg!(target_os = "windows") {
        assert_eq!(candidates.separator, ';');
        assert_eq!(candidates.directory_separator, '\\');
        assert_eq!(candidates.extensions, &[".exe"]);
    } else {
        assert_eq!(candidates.separator, ':');
        assert_eq!(candidates.directory_separator, '/');
        assert_eq!(candidates.extensions, &[""]);
    }
    if cfg!(target_os = "macos") {
        assert!(
            candidates
                .well_known
                .iter()
                .any(|path| path.contains(".app/"))
        );
    }
    assert!(!candidates.on_path.is_empty());
}
