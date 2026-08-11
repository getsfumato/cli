//! Chromium-family browser discovery.
//!
//! Slides, pages, documents and diagrams all need a browser, so this does not
//! belong to any one renderer. Resolution has four steps, and the first hit
//! wins:
//!
//! 1. the path the user configured, which is an error when it does not exist
//!    rather than a silent fall through to a guess;
//! 2. environment variables, including the two the tools this crate shells out
//!    to already read;
//! 3. `PATH`, which is how a browser is normally found on Linux and Windows;
//! 4. well-known absolute locations, which is the only way to find a macOS
//!    application bundle because a bundle is never on `PATH`.
//!
//! That last step used to be the whole implementation, which is why it worked on
//! macOS and could not work anywhere else. It is a last resort, not a strategy.
//!
//! The candidate tables are data and the filesystem and environment arrive
//! through [`Host`], so the ordering is testable for every operating system from
//! any one of them — the property that was previously impossible to assert.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Result, bail};

/// Environment variables consulted, in order of precedence.
///
/// `SFUMATO_BROWSER` is ours. The other two belong to the tools this crate
/// drives — puppeteer, behind `marp`, reads `PUPPETEER_EXECUTABLE_PATH`, and
/// `mmdc` reads `CHROME_PATH` — so honouring them means a machine already set up
/// for those tools does not have to answer the same question twice.
const ENVIRONMENT_VARIABLES: &[&str] = &[
    "SFUMATO_BROWSER",
    "PUPPETEER_EXECUTABLE_PATH",
    "CHROME_PATH",
];

/// Where to look for a browser on one operating system.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Candidates {
    /// Executable names to resolve against `PATH`, in preference order.
    pub(crate) on_path: &'static [&'static str],
    /// Absolute locations to probe once `PATH` has come up empty.
    pub(crate) well_known: &'static [&'static str],
    /// The `PATH` entry separator.
    ///
    /// Part of the table rather than read from `cfg!` so that the Windows lookup
    /// is exercisable from a unix host.
    pub(crate) separator: char,
    /// The separator between a directory and a file name.
    ///
    /// Also in the table, and for a sharper reason: `Path::join` uses the
    /// separator of the platform the binary was *compiled* for, so building
    /// candidates with it would make this function behave differently under test
    /// than in production and leave the Windows path shape unassertable.
    pub(crate) directory_separator: char,
    /// Suffixes to append to each name on `PATH`. Unix needs none.
    pub(crate) extensions: &'static [&'static str],
}

/// A macOS bundle is not on `PATH`, so only `well_known` can find one.
pub(crate) const MACOS: Candidates = Candidates {
    on_path: &["google-chrome", "chromium", "microsoft-edge"],
    well_known: &[
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    ],
    separator: ':',
    directory_separator: '/',
    extensions: &[""],
};

pub(crate) const LINUX: Candidates = Candidates {
    on_path: &[
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "microsoft-edge",
        "brave-browser",
    ],
    well_known: &[
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/snap/bin/chromium",
        "/var/lib/flatpak/exports/bin/org.chromium.Chromium",
    ],
    separator: ':',
    directory_separator: '/',
    extensions: &[""],
};

pub(crate) const WINDOWS: Candidates = Candidates {
    on_path: &["chrome", "msedge", "chromium"],
    well_known: &[
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ],
    separator: ';',
    directory_separator: '\\',
    extensions: &[".exe"],
};

/// The table for the operating system this is running on.
///
/// A match on `std::env::consts::OS` rather than `#[cfg]` branches, so that all
/// three tables belong to one code path that compiles everywhere. Under `cfg`,
/// two of them would be dead code on any given host and the compiler would stop
/// checking them — which is the shape of mistake that let a macOS-only
/// implementation ship in the first place.
pub(crate) fn host_candidates() -> Candidates {
    match std::env::consts::OS {
        "macos" => MACOS,
        "windows" => WINDOWS,
        _ => LINUX,
    }
}

/// The environment and filesystem a lookup runs against.
///
/// Injected so a test can describe a machine it is not running on.
pub(crate) trait Host {
    fn variable(&self, key: &str) -> Option<OsString>;
    fn is_file(&self, path: &Path) -> bool;
}

/// The real machine.
pub(crate) struct RealHost;

impl Host for RealHost {
    fn variable(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// Walks the four steps and returns the first browser that exists.
pub(crate) fn detect(host: &dyn Host, candidates: &Candidates) -> Option<PathBuf> {
    for key in ENVIRONMENT_VARIABLES {
        // An empty value is how a shell unsets a variable in practice, so treat it
        // as absent rather than as the path "".
        let Some(value) = host.variable(key).filter(|value| !value.is_empty()) else {
            continue;
        };
        let path = PathBuf::from(value);
        if host.is_file(&path) {
            return Some(path);
        }
    }

    if let Some(path) = host.variable("PATH") {
        // Lossy is right here: a directory whose name is not UTF-8 cannot match
        // any of the ASCII names being looked for anyway.
        for directory in path.to_string_lossy().split(candidates.separator) {
            if directory.is_empty() {
                continue;
            }
            let separator = candidates.directory_separator;
            let prefix = directory.trim_end_matches(separator);
            for name in candidates.on_path {
                for extension in candidates.extensions {
                    let candidate = PathBuf::from(format!("{prefix}{separator}{name}{extension}"));
                    if host.is_file(&candidate) {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    candidates
        .well_known
        .iter()
        .map(Path::new)
        .find(|path| host.is_file(path))
        .map(Path::to_path_buf)
}

/// Detects once per process: a browser does not appear mid-run, and the scan is
/// a burst of syscalls that every renderer would otherwise repeat.
fn detected() -> Option<&'static Path> {
    static DETECTED: OnceLock<Option<PathBuf>> = OnceLock::new();
    DETECTED
        .get_or_init(|| detect(&RealHost, &host_candidates()))
        .as_deref()
}

/// Resolves the browser to launch, preferring what the user configured.
pub(crate) fn resolve(configured: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(path) = configured {
        if !path.is_file() {
            bail!(
                "Configured browser path does not exist or is not a file: {}",
                path.display()
            );
        }
        return Ok(Some(path.to_path_buf()));
    }
    Ok(detected().map(Path::to_path_buf))
}

/// The message for a machine with no browser at all.
///
/// `marp.rs`, `pages.rs` and `renderers/document.rs` each classify a missing
/// browser as retryable by matching the `Could not find Chrome` prefix, so that
/// prefix is load-bearing: losing it turns "unavailable" into "permanent" and
/// the operation stops being retried. Built here so the remedy is written once
/// and the prefix has one owner.
pub(crate) fn not_found(purpose: &str) -> String {
    format!(
        "Could not find Chrome, Chromium, or Edge {purpose}. Install one, or name \
         the executable with `browser.path` in your configuration or the \
         SFUMATO_BROWSER environment variable."
    )
}

#[cfg(test)]
#[path = "../tests/unit/browser.rs"]
mod tests;
