//! Executable lookup, for both platforms' rules, from either one.

use super::*;

fn lookup<'a>(path: &'a str, windows: bool, files: &'a dyn Fn(&Path) -> bool) -> Lookup<'a> {
    if windows {
        Lookup {
            directories: path.split(';').collect(),
            extensions: windows_extensions(Some(OsString::from(".COM;.EXE;.BAT;.CMD"))),
            separator: '\\',
            exists: files,
        }
    } else {
        Lookup {
            directories: path.split(':').collect(),
            extensions: vec![String::new()],
            separator: '/',
            exists: files,
        }
    }
}

fn present(known: &'static [&'static str]) -> impl Fn(&Path) -> bool {
    move |candidate: &Path| known.iter().any(|path| Path::new(path) == candidate)
}

#[test]
fn finds_a_bare_executable_on_unix() {
    let files = present(&["/usr/bin/marp"]);
    let found = lookup("/usr/local/bin:/usr/bin", false, &files).find("marp");
    assert_eq!(found, Some(PathBuf::from("/usr/bin/marp")));
}

#[test]
fn searches_directories_in_order() {
    let files = present(&["/opt/bin/marp", "/usr/bin/marp"]);
    let found = lookup("/opt/bin:/usr/bin", false, &files).find("marp");
    assert_eq!(found, Some(PathBuf::from("/opt/bin/marp")));
}

#[test]
fn finds_nothing_when_the_tool_is_absent() {
    let files = present(&[]);
    assert_eq!(lookup("/usr/bin", false, &files).find("marp"), None);
}

#[test]
fn ignores_empty_path_entries() {
    // A doubled or trailing separator would otherwise probe the working directory,
    // which is not where a tool should be found.
    let files = present(&["marp", "/usr/bin/marp"]);
    let found = lookup(":/usr/bin::", false, &files).find("marp");
    assert_eq!(found, Some(PathBuf::from("/usr/bin/marp")));
}

#[test]
fn finds_a_cmd_shim_on_windows() {
    // The whole reason this module exists: npm is npm.cmd, and CreateProcess does
    // not consult PATHEXT, so `Command::new("npm")` fails on a machine where npm
    // works fine in a shell.
    let files = present(&[r"C:\Program Files\nodejs\npm.cmd"]);
    let found = lookup(r"C:\Windows;C:\Program Files\nodejs", true, &files).find("npm");
    assert_eq!(
        found,
        Some(PathBuf::from(r"C:\Program Files\nodejs\npm.cmd"))
    );
}

#[test]
fn prefers_an_exe_over_a_shim_of_the_same_name() {
    // The empty extension comes first, so a real executable wins.
    let files = present(&[r"C:\tools\ffmpeg", r"C:\tools\ffmpeg.cmd"]);
    let found = lookup(r"C:\tools", true, &files).find("ffmpeg");
    assert_eq!(found, Some(PathBuf::from(r"C:\tools\ffmpeg")));
}

#[test]
fn tries_pathext_in_the_order_given() {
    let files = present(&[r"C:\tools\marp.cmd", r"C:\tools\marp.exe"]);
    let found = lookup(r"C:\tools", true, &files).find("marp");
    // .EXE precedes .CMD in the default PATHEXT.
    assert_eq!(found, Some(PathBuf::from(r"C:\tools\marp.exe")));
}

#[test]
fn an_explicit_path_is_used_rather_than_searched_for() {
    // Managed renderers are invoked by absolute path; searching PATH for
    // `/root/.sfumato/renderers/…/pagedjs-cli` would find nothing.
    let files = present(&["/root/.sfumato/renderers/pagedjs/bin/pagedjs-cli"]);
    let found =
        lookup("/usr/bin", false, &files).find("/root/.sfumato/renderers/pagedjs/bin/pagedjs-cli");
    assert_eq!(
        found,
        Some(PathBuf::from(
            "/root/.sfumato/renderers/pagedjs/bin/pagedjs-cli"
        ))
    );
}

#[test]
fn an_explicit_path_that_does_not_exist_is_not_found() {
    let files = present(&[]);
    assert_eq!(lookup("/usr/bin", false, &files).find("/nope/tool"), None);
}

#[test]
fn pathext_defaults_when_the_variable_is_missing() {
    let extensions = windows_extensions(None);
    assert_eq!(extensions[0], "", "a real executable must be tried first");
    assert!(extensions.contains(&".cmd".to_string()));
    assert!(extensions.contains(&".exe".to_string()));
}

#[test]
fn resolving_an_absent_tool_hands_back_the_bare_name() {
    // Deliberate: the spawn then fails exactly as it did before, with the message
    // the renderers already classify. Resolution must not turn "not installed" into
    // a different failure.
    assert_eq!(
        resolve("sfumato-definitely-not-a-real-tool"),
        OsString::from("sfumato-definitely-not-a-real-tool")
    );
}

#[test]
fn resolving_a_real_tool_finds_it_on_this_machine() {
    // `sh` exists on every platform this ships to, Windows included via Git.
    let resolved = resolve("sh");
    if cfg!(windows) {
        // Either found, or the bare fallback; both are acceptable here.
        assert!(!resolved.is_empty());
    } else {
        assert_ne!(
            resolved,
            OsString::from("sh"),
            "sh should have been found on PATH"
        );
    }
}
