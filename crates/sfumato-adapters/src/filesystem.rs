//! Local workspace filesystem adapter.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use sfumato_core::filesystem::{TemporaryWorkspace, WorkspaceEntry, WorkspaceFileSystem};

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
    fn temporary_directory(&self, prefix: &str) -> Result<Box<dyn TemporaryWorkspace>> {
        let directory = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir()
            .context("Could not create temporary workspace")?;
        Ok(Box::new(LocalTemporaryWorkspace(directory)))
    }

    fn canonicalize(&self, path: &Path) -> Result<PathBuf> {
        fs::canonicalize(path).with_context(|| format!("Could not resolve {}", path.display()))
    }

    fn read_text(&self, path: &Path) -> Result<String> {
        fs::read_to_string(path).with_context(|| format!("Could not read {}", path.display()))
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path).with_context(|| format!("Could not create {}", path.display()))
    }

    fn write(&self, path: &Path, contents: &[u8]) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Could not create {}", parent.display()))?;
        }
        fs::write(path, contents).with_context(|| format!("Could not write {}", path.display()))
    }

    fn copy_file(&self, source: &Path, destination: &Path) -> Result<()> {
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
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<WorkspaceEntry>> {
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
    }

    fn copy_tree(&self, source: &Path, destination: &Path, exclude: &[&str]) -> Result<()> {
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
    }

    fn list_files(&self, root: &Path, exclude: &[&str]) -> Result<Vec<PathBuf>> {
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
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("Could not remove {}", path.display()))?;
        }
        Ok(())
    }

    fn publish_atomic(&self, source: &Path, destination_dir: &Path) -> Result<PathBuf> {
        let filename = source
            .file_name()
            .context("Published artifact must have a filename")?;
        fs::create_dir_all(destination_dir)
            .with_context(|| format!("Could not create {}", destination_dir.display()))?;
        let destination = destination_dir.join(filename);
        if source != destination {
            let mut input = fs::File::open(source)
                .with_context(|| format!("Could not open {} for publishing", source.display()))?;
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
    }
}

#[cfg(test)]
#[path = "../tests/unit/filesystem.rs"]
mod tests;
