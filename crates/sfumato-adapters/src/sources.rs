//! Filesystem-backed source discovery and project instruction loading.

use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use sfumato_core::{
    errors::{ErrorClass, SfumatoError, SfumatoResult},
    sources::{ProjectInstructions, SourceDocument, SourceReader},
};
use walkdir::WalkDir;

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "md", "txt", "rs", "py", "js", "ts", "html", "css", "json", "toml", "yaml", "yml",
];
const MAX_SOURCE_FILES: usize = 256;
/// Longest a single UTF-8 character can be.
///
/// The byte cap can slice one character in half, so discarding up to this many
/// bytes is the intended repair; more than that is not a cut character.
const MAX_UTF8_CHARACTER_BYTES: usize = 4;
const MAX_SOURCE_BYTES_PER_FILE: u64 = 1_048_576;
const MAX_SOURCE_TOTAL_BYTES: u64 = 16_777_216;
const PROJECT_INSTRUCTIONS_FILE: &str = "SFUMATO.md";
const MAX_PROJECT_INSTRUCTIONS_BYTES: u64 = 64 * 1024;

/// Local source reader with deterministic aggregate budgets.
#[derive(Clone, Copy, Debug, Default)]
pub struct FilesystemSourceReader;

impl SourceReader for FilesystemSourceReader {
    fn collect(&self, inputs: &[PathBuf]) -> SfumatoResult<Vec<SourceDocument>> {
        let result: Result<Vec<SourceDocument>> = (|| {
            let mut documents = Vec::new();
            let mut total_bytes = 0_u64;
            let mut seen = BTreeSet::new();
            for input in inputs {
                if input.is_file() {
                    push_source_file(input, &mut documents, &mut total_bytes, &mut seen)?;
                } else if input.is_dir() {
                    for entry in WalkDir::new(input) {
                        let entry = entry.with_context(|| {
                            format!("Could not traverse source directory {}", input.display())
                        })?;
                        if entry.file_type().is_file() {
                            push_source_file(
                                entry.path(),
                                &mut documents,
                                &mut total_bytes,
                                &mut seen,
                            )?;
                        }
                    }
                } else {
                    bail!("Input path does not exist: {}", input.display());
                }
            }
            documents.sort_by(|left, right| left.path.cmp(&right.path));
            Ok(documents)
        })();
        source_result(result)
    }

    fn project_instructions(
        &self,
        project_root: &Path,
    ) -> SfumatoResult<Option<ProjectInstructions>> {
        let result: Result<Option<ProjectInstructions>> = (|| {
            let path = project_root.join(PROJECT_INSTRUCTIONS_FILE);
            if !path
                .try_exists()
                .with_context(|| format!("Could not inspect {}", path.display()))?
            {
                return Ok(None);
            }
            let canonical_root = project_root.canonicalize().with_context(|| {
                format!("Could not resolve project root {}", project_root.display())
            })?;
            let canonical_path = path
                .canonicalize()
                .with_context(|| format!("Could not resolve {}", path.display()))?;
            if !canonical_path.starts_with(&canonical_root) {
                bail!(
                    "Project instructions {} resolve outside project root {}",
                    path.display(),
                    project_root.display()
                );
            }
            let metadata = fs::metadata(&canonical_path)
                .with_context(|| format!("Could not inspect {}", path.display()))?;
            if !metadata.is_file() {
                bail!("Project instructions path {} is not a file", path.display());
            }
            if metadata.len() > MAX_PROJECT_INSTRUCTIONS_BYTES {
                bail!(
                    "Project instructions {} are {} bytes; the maximum is {} bytes",
                    path.display(),
                    metadata.len(),
                    MAX_PROJECT_INSTRUCTIONS_BYTES
                );
            }
            let content = fs::read_to_string(&canonical_path).with_context(|| {
                format!("Could not read project instructions {}", path.display())
            })?;
            Ok(Some(ProjectInstructions {
                path,
                content: content.trim().to_string(),
            }))
        })();
        source_result(result)
    }
}

fn source_result<T>(result: Result<T>) -> SfumatoResult<T> {
    result.map_err(|error| {
        SfumatoError::new(
            sfumato_core::errors::ErrorCode::Validation,
            ErrorClass::Permanent,
            format!("{error:#}"),
        )
    })
}

fn push_source_file(
    path: &Path,
    documents: &mut Vec<SourceDocument>,
    total_bytes: &mut u64,
    seen: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    if !is_supported(path) {
        return Ok(());
    }
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("Could not resolve source {}", path.display()))?;
    if !seen.insert(canonical.clone()) {
        return Ok(());
    }
    if documents.len() >= MAX_SOURCE_FILES {
        bail!(
            "Source selection exceeds the maximum of {MAX_SOURCE_FILES} supported files; select a narrower directory or explicit files"
        );
    }
    let bytes = fs::metadata(&canonical)
        .with_context(|| format!("Could not inspect {}", canonical.display()))?
        .len();
    *total_bytes = total_bytes.saturating_add(bytes.min(MAX_SOURCE_BYTES_PER_FILE));
    if *total_bytes > MAX_SOURCE_TOTAL_BYTES {
        bail!(
            "Source selection exceeds Sfumato's {} MiB preflight budget",
            MAX_SOURCE_TOTAL_BYTES / 1_048_576
        );
    }
    let mut raw = Vec::new();
    fs::File::open(&canonical)
        .with_context(|| format!("Could not open {}", canonical.display()))?
        .take(MAX_SOURCE_BYTES_PER_FILE + 1)
        .read_to_end(&mut raw)
        .with_context(|| format!("Could not read {}", canonical.display()))?;
    let truncated = raw.len() as u64 > MAX_SOURCE_BYTES_PER_FILE;
    raw.truncate(MAX_SOURCE_BYTES_PER_FILE as usize);
    // Trimming here repairs one multibyte character that the byte cap cut in
    // half, which is at most `MAX_UTF8_CHARACTER_BYTES - 1` bytes. It used to pop
    // until the buffer parsed no matter how much that took, so a file with a text
    // extension and binary content had everything after the first invalid byte
    // discarded — 96% of a 52 KB file in the reported case — and handed to the
    // model as though it were the whole file. `truncated` is computed before this
    // loop and only tracks the size cap, so the marker never covered these losses.
    let capped = raw.len();
    while std::str::from_utf8(&raw).is_err() && !raw.is_empty() {
        raw.pop();
    }
    let discarded = capped - raw.len();
    if discarded >= MAX_UTF8_CHARACTER_BYTES {
        // The error the original code drafted on the next line, which the pop loop
        // had made unreachable by guaranteeing the conversion would succeed.
        // Refused rather than marked: this runs in preflight, before any model call
        // is spent, and a text-extensioned file that is mostly binary is not what
        // the caller meant to include.
        bail!(
            "Source {} is not valid UTF-8: {discarded} of {capped} bytes are not text. \
             Exclude the file or convert it before generating.",
            canonical.display()
        );
    }
    let mut content = String::from_utf8(raw)
        .with_context(|| format!("Source {} is not valid UTF-8", canonical.display()))?;
    if truncated {
        content.push_str("\n[...source file truncated by sfumato preflight...]");
    }
    documents.push(SourceDocument {
        path: canonical,
        content,
    });
    Ok(())
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

#[cfg(test)]
#[path = "../tests/unit/sources.rs"]
mod tests;
