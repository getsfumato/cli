use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn dependency_names(manifest: &Path) -> BTreeSet<String> {
    let contents = fs::read_to_string(manifest)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", manifest.display()));
    let document = toml::from_str::<toml::Value>(&contents)
        .unwrap_or_else(|error| panic!("could not parse {}: {error}", manifest.display()));
    ["dependencies", "dev-dependencies", "build-dependencies"]
        .into_iter()
        .filter_map(|section| document.get(section).and_then(toml::Value::as_table))
        .flat_map(|table| table.keys().cloned())
        .collect()
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", directory.display()));
        for entry in entries {
            let path = entry.expect("directory entry should be readable").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
                sources.push(path);
            }
        }
    }
    sources.sort();
    sources
}

fn assert_dependencies_exclude(manifest: &Path, forbidden: &[&str]) {
    let dependencies = dependency_names(manifest);
    let violations = forbidden
        .iter()
        .filter(|name| dependencies.contains(**name))
        .copied()
        .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "{} contains forbidden dependencies: {}",
        manifest.display(),
        violations.join(", ")
    );
}

fn assert_sources_exclude(root: &Path, forbidden: &[&str]) {
    let mut violations = Vec::new();
    for source in rust_sources(root) {
        let contents = fs::read_to_string(&source)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", source.display()));
        for token in forbidden {
            if contents.contains(token) {
                violations.push(format!("{} contains `{token}`", source.display()));
            }
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn domain_remains_pure_and_dependency_free_from_outer_layers() {
    let root = workspace_root().join("crates/sfumato-domain");
    assert_dependencies_exclude(
        &root.join("Cargo.toml"),
        &[
            "anyhow",
            "async-trait",
            "clap",
            "indicatif",
            "inquire",
            "ratatui",
            "reqwest",
            "sfumato-adapters",
            "sfumato-core",
            "tokio",
        ],
    );
    assert_sources_exclude(
        &root.join("src"),
        &[
            "std::fs",
            "std::net",
            "std::process",
            "sfumato_adapters",
            "sfumato_core",
            "tokio::",
        ],
    );
}

#[test]
fn core_owns_policy_without_presentation_or_concrete_infrastructure() {
    let root = workspace_root().join("crates/sfumato-core");
    assert_dependencies_exclude(
        &root.join("Cargo.toml"),
        &[
            "anyhow",
            "clap",
            "indicatif",
            "inquire",
            "ratatui",
            "reqwest",
            "sfumato-adapters",
        ],
    );
    assert_sources_exclude(
        &root.join("src"),
        &[
            "anyhow::",
            "clap::",
            "eprintln!",
            "indicatif::",
            "inquire::",
            "println!",
            "ratatui::",
            "reqwest::",
            "sfumato_adapters",
            "tokio::",
        ],
    );
}

#[test]
fn adapter_dependency_direction_points_inward() {
    let root = workspace_root().join("crates/sfumato-adapters");
    let dependencies = dependency_names(&root.join("Cargo.toml"));
    assert!(dependencies.contains("sfumato-core"));
    assert!(!dependencies.contains("sfumato"));
    assert!(!dependencies.contains("clap"));
    assert!(!dependencies.contains("indicatif"));
    assert!(!dependencies.contains("inquire"));
    assert!(!dependencies.contains("ratatui"));
}

/// Every HTTP client must carry its own request timeout.
///
/// The CLI and TUI both run catalog and status reads with a context that may
/// carry no deadline, so a client without a timeout has no bound at all: a port
/// that accepts the connection and never replies hangs the process forever. One
/// of five clients was built with a bare `Client::new()`, so this guards the
/// property across the tree rather than at the site that happened to be missing.
#[test]
fn every_http_client_declares_a_request_timeout() {
    let mut violations = Vec::new();
    for source in rust_sources(&workspace_root().join("crates")) {
        let contents = fs::read_to_string(&source)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", source.display()));
        // A bare constructor cannot carry a timeout, so it is always a violation.
        if contents.contains("Client::new()") {
            violations.push(format!("{}: Client::new()", source.display()));
        }
        // A builder can, but only if it says so somewhere in the same file.
        if contents.contains("Client::builder()") && !contents.contains(".timeout(") {
            violations.push(format!(
                "{}: Client::builder() without .timeout()",
                source.display()
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "HTTP clients without a request timeout:\n  {}",
        violations.join("\n  ")
    );
}
