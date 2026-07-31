//! Local workspace filesystem adapter.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use sfumato_core::{
    errors::{ErrorClass, SfumatoError, SfumatoResult},
    filesystem::{TemporaryWorkspace, WorkspaceEntry, WorkspaceFileSystem},
};

/// Production filesystem operations for resource workflows.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalWorkspaceFileSystem;

struct LocalTemporaryWorkspace(tempfile::TempDir);

impl TemporaryWorkspace for LocalTemporaryWorkspace {
    fn path(&self) -> &Path {
        self.0.path()
    }
}

impl WorkspaceFileSystem for LocalWorkspaceFileSystem {
    fn temporary_directory(&self, prefix: &str) -> SfumatoResult<Box<dyn TemporaryWorkspace>> {
        workspace_result((|| {
            let directory = tempfile::Builder::new()
                .prefix(prefix)
                .tempdir()
                .context("Could not create temporary workspace")?;
            Ok(Box::new(LocalTemporaryWorkspace(directory)) as Box<dyn TemporaryWorkspace>)
        })())
    }

    fn canonicalize(&self, path: &Path) -> SfumatoResult<PathBuf> {
        workspace_result(
            fs::canonicalize(path).with_context(|| format!("Could not resolve {}", path.display())),
        )
    }

    fn read_text(&self, path: &Path) -> SfumatoResult<String> {
        workspace_result(
            fs::read_to_string(path).with_context(|| format!("Could not read {}", path.display())),
        )
    }

    fn create_dir_all(&self, path: &Path) -> SfumatoResult<()> {
        workspace_result(
            fs::create_dir_all(path)
                .with_context(|| format!("Could not create {}", path.display())),
        )
    }

    fn write(&self, path: &Path, contents: &[u8]) -> SfumatoResult<()> {
        workspace_result((|| {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Could not create {}", parent.display()))?;
            }
            fs::write(path, contents).with_context(|| format!("Could not write {}", path.display()))
        })())
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> SfumatoResult<()> {
        workspace_result((|| {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Could not create {}", parent.display()))?;
            }
            fs::copy(source, destination).with_context(|| {
                format!(
                    "Could not copy {} to {}",
                    source.display(),
                    destination.display()
                )
            })?;
            Ok(())
        })())
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read_dir(&self, path: &Path) -> SfumatoResult<Vec<WorkspaceEntry>> {
        workspace_result((|| {
            let mut entries = Vec::new();
            for entry in
                fs::read_dir(path).with_context(|| format!("Could not read {}", path.display()))?
            {
                let entry = entry?;
                let file_type = entry.file_type()?;
                if file_type.is_symlink() {
                    bail!(
                        "Workspace contains an unsafe symlink: {}",
                        entry.path().display()
                    );
                }
                entries.push(WorkspaceEntry {
                    path: entry.path(),
                    is_file: file_type.is_file(),
                    is_directory: file_type.is_dir(),
                });
            }
            entries.sort_by(|left, right| left.path.cmp(&right.path));
            Ok(entries)
        })())
    }

    fn copy_tree(&self, source: &Path, destination: &Path, exclude: &[&str]) -> SfumatoResult<()> {
        workspace_result((|| {
            fs::create_dir_all(destination)
                .with_context(|| format!("Could not create {}", destination.display()))?;
            for entry in walkdir::WalkDir::new(source).follow_links(false) {
                let entry = entry?;
                if entry.file_type().is_symlink() {
                    bail!(
                        "Workspace contains an unsafe symlink: {}",
                        entry.path().display()
                    );
                }
                let relative = entry.path().strip_prefix(source)?;
                if relative.as_os_str().is_empty() {
                    continue;
                }
                if entry.file_type().is_file()
                    && entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| exclude.contains(&name))
                {
                    continue;
                }
                let target = destination.join(relative);
                if entry.file_type().is_dir() {
                    fs::create_dir_all(&target)?;
                } else if entry.file_type().is_file() {
                    fs::copy(entry.path(), target)?;
                }
            }
            Ok(())
        })())
    }

    fn list_files(&self, root: &Path, exclude: &[&str]) -> SfumatoResult<Vec<PathBuf>> {
        workspace_result((|| {
            let mut files = Vec::new();
            for entry in walkdir::WalkDir::new(root).follow_links(false) {
                let entry = entry?;
                if entry.file_type().is_symlink() {
                    bail!(
                        "Workspace contains an unsafe symlink: {}",
                        entry.path().display()
                    );
                }
                if entry.file_type().is_file()
                    && !entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| exclude.contains(&name))
                {
                    files.push(entry.path().to_path_buf());
                }
            }
            files.sort();
            Ok(files)
        })())
    }

    fn remove_file(&self, path: &Path) -> SfumatoResult<()> {
        workspace_result((|| {
            if path.exists() {
                fs::remove_file(path)
                    .with_context(|| format!("Could not remove {}", path.display()))?;
            }
            Ok(())
        })())
    }

    fn publish_atomic(&self, source: &Path, destination_dir: &Path) -> SfumatoResult<PathBuf> {
        workspace_result((|| {
            let filename = source
                .file_name()
                .context("Published artifact must have a filename")?;
            fs::create_dir_all(destination_dir)
                .with_context(|| format!("Could not create {}", destination_dir.display()))?;
            let destination = destination_dir.join(filename);
            if source != destination {
                let mut input = fs::File::open(source).with_context(|| {
                    format!("Could not open {} for publishing", source.display())
                })?;
                let mut temporary =
                    tempfile::NamedTempFile::new_in(destination_dir).with_context(|| {
                        format!(
                            "Could not create a temporary published artifact in {}",
                            destination_dir.display()
                        )
                    })?;
                io::copy(&mut input, &mut temporary)?;
                temporary.as_file().sync_all()?;
                temporary
                    .persist(&destination)
                    .map_err(|error| error.error)
                    .with_context(|| {
                        format!("Could not atomically publish {}", destination.display())
                    })?;
            }
            Ok(destination)
        })())
    }

    fn publish_tree_atomic(&self, source: &Path, destination: &Path) -> SfumatoResult<PathBuf> {
        workspace_result((|| {
            if !source.is_dir() {
                bail!(
                    "Published page payload is not a directory: {}",
                    source.display()
                );
            }
            let parent = destination
                .parent()
                .context("Published directory must have a parent")?;
            fs::create_dir_all(parent)
                .with_context(|| format!("Could not create {}", parent.display()))?;
            let temporary = tempfile::Builder::new()
                .prefix(".sfumato-publish-")
                .tempdir_in(parent)
                .with_context(|| format!("Could not stage publication in {}", parent.display()))?;
            let staged = temporary.path().join("payload");
            self.copy_tree(source, &staged, &[])
                .map_err(anyhow::Error::from)?;
            let backup = parent.join(format!(
                ".{}.sfumato-backup-{}",
                destination
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("page"),
                std::process::id()
            ));
            if backup.exists() {
                fs::remove_dir_all(&backup)
                    .with_context(|| format!("Could not remove stale {}", backup.display()))?;
            }
            let had_destination = destination.exists();
            if had_destination {
                if !destination.is_dir() {
                    bail!(
                        "Published page destination is not a directory: {}",
                        destination.display()
                    );
                }
                fs::rename(destination, &backup).with_context(|| {
                    format!(
                        "Could not stage existing publication {}",
                        destination.display()
                    )
                })?;
            }
            if let Err(error) = fs::rename(&staged, destination) {
                if had_destination {
                    let _ = fs::rename(&backup, destination);
                }
                return Err(error).with_context(|| {
                    format!("Could not atomically publish {}", destination.display())
                });
            }
            if had_destination {
                fs::remove_dir_all(&backup)
                    .with_context(|| format!("Could not remove {}", backup.display()))?;
            }
            Ok(destination.to_path_buf())
        })())
    }

    fn remove_tree(&self, path: &Path) -> SfumatoResult<()> {
        workspace_result((|| {
            if !path.exists() {
                return Ok(());
            }
            let metadata = fs::symlink_metadata(path)
                .with_context(|| format!("Could not inspect {}", path.display()))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                bail!(
                    "Refusing to remove non-directory publication {}",
                    path.display()
                );
            }
            fs::remove_dir_all(path).with_context(|| format!("Could not remove {}", path.display()))
        })())
    }
}

fn workspace_result<T>(result: Result<T>) -> SfumatoResult<T> {
    result.map_err(|error| SfumatoError::artifact(ErrorClass::Permanent, format_args!("{error:#}")))
}

#[cfg(test)]
#[path = "../tests/unit/filesystem.rs"]
mod tests;
