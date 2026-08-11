//! Finding an external tool on `PATH`.
//!
//! `Command::new("npm")` is enough on unix and not on Windows, where npm installs
//! as `npm.cmd`: `CreateProcess` does not consult `PATHEXT`, so the spawn fails
//! with "cannot find the file specified" even though the tool is on `PATH` and
//! works in a shell. Every tool this crate drives through a node wrapper — `npm`,
//! `marp`, `mmdc`, `pagedjs-cli` — has that shape.
//!
//! Resolution is best-effort on purpose. When nothing is found the bare name is
//! handed back so the spawn fails exactly as it did before, with the error message
//! the renderers already recognise; this must not turn "not installed" into a
//! different failure.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// How to read `PATH` and probe the filesystem, injected so the Windows lookup is
/// testable from a unix host and vice versa.
pub(crate) struct Lookup<'a> {
    /// `PATH`, already split by the caller's platform separator.
    pub(crate) directories: Vec<&'a str>,
    /// Suffixes to try, in order. Unix uses `[""]`; Windows needs `PATHEXT`.
    pub(crate) extensions: Vec<String>,
    /// The separator joining a directory to a file name.
    pub(crate) separator: char,
    /// Whether a candidate exists.
    pub(crate) exists: &'a dyn Fn(&Path) -> bool,
}

impl Lookup<'_> {
    /// The first candidate that exists, if any.
    pub(crate) fn find(&self, name: &str) -> Option<PathBuf> {
        // An explicit path is not a lookup: `~/.sfumato/renderers/…/pagedjs-cli`
        // must not be searched for on PATH.
        if name.contains('/') || name.contains('\\') {
            let direct = PathBuf::from(name);
            return (self.exists)(&direct).then_some(direct);
        }

        for directory in &self.directories {
            if directory.is_empty() {
                continue;
            }
            let prefix = directory.trim_end_matches(self.separator);
            for extension in &self.extensions {
                let candidate =
                    PathBuf::from(format!("{prefix}{}{name}{extension}", self.separator));
                if (self.exists)(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }
}

/// `PATHEXT`, lowercased, with the empty extension first so a real executable
/// beats a wrapper of the same name.
fn windows_extensions(path_ext: Option<OsString>) -> Vec<String> {
    let raw = path_ext.unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
    let mut extensions = vec![String::new()];
    extensions.extend(
        raw.to_string_lossy()
            .split(';')
            .filter(|entry| !entry.is_empty())
            .map(|entry| entry.to_lowercase()),
    );
    extensions
}

fn host_lookup<'a>(path: &'a str, exists: &'a dyn Fn(&Path) -> bool) -> Lookup<'a> {
    if cfg!(windows) {
        Lookup {
            directories: path.split(';').collect(),
            extensions: windows_extensions(std::env::var_os("PATHEXT")),
            separator: '\\',
            exists,
        }
    } else {
        Lookup {
            directories: path.split(':').collect(),
            extensions: vec![String::new()],
            separator: '/',
            exists,
        }
    }
}

/// Resolves a tool name to something spawnable.
///
/// Falls back to the name itself when nothing is found, so a missing tool fails at
/// spawn exactly as before rather than through a new error path.
pub(crate) fn resolve(name: &str) -> OsString {
    let Some(path) = std::env::var_os("PATH") else {
        return OsString::from(name);
    };
    let path = path.to_string_lossy().into_owned();
    let exists = |candidate: &Path| candidate.is_file();
    host_lookup(&path, &exists)
        .find(name)
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from(name))
}

#[cfg(test)]
#[path = "../tests/unit/executables.rs"]
mod tests;
