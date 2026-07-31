//! Filesystem operations required by application workflows.

use std::path::{Path, PathBuf};

use crate::errors::SfumatoResult;

/// One non-symlink directory entry returned by a workspace adapter.
#[derive(Clone, Debug)]
pub struct WorkspaceEntry {
    /// Absolute or adapter-resolved path.
    pub path: PathBuf,
    /// Whether the entry is a regular file.
    pub is_file: bool,
    /// Whether the entry is a directory.
    pub is_directory: bool,
}

/// Automatically cleaned operation-scoped temporary directory.
pub trait TemporaryWorkspace: Send {
    /// Root path available for renderer and workflow files.
    fn path(&self) -> &Path;
}

/// Port for local files manipulated by generation and editing workflows.
pub trait WorkspaceFileSystem: Send + Sync {
    /// Creates an automatically cleaned temporary directory.
    fn temporary_directory(&self, prefix: &str) -> SfumatoResult<Box<dyn TemporaryWorkspace>>;
    /// Resolves an existing path and rejects invalid filesystem state.
    fn canonicalize(&self, path: &Path) -> SfumatoResult<PathBuf>;
    /// Reads a UTF-8 text file.
    fn read_text(&self, path: &Path) -> SfumatoResult<String>;
    /// Creates a directory and its parents.
    fn create_dir_all(&self, path: &Path) -> SfumatoResult<()>;
    /// Writes a complete file, creating its parent directory when needed.
    fn write(&self, path: &Path, contents: &[u8]) -> SfumatoResult<()>;
    /// Copies one regular file, creating the destination parent when needed.
    fn copy_file(&self, source: &Path, destination: &Path) -> SfumatoResult<()>;
    /// Returns whether a path is a regular file.
    fn is_file(&self, path: &Path) -> bool;
    /// Lists direct children and rejects symbolic links.
    fn read_dir(&self, path: &Path) -> SfumatoResult<Vec<WorkspaceEntry>>;
    /// Recursively copies a directory while excluding files by basename.
    fn copy_tree(&self, source: &Path, destination: &Path, exclude: &[&str]) -> SfumatoResult<()>;
    /// Recursively lists regular files and rejects symbolic links.
    fn list_files(&self, root: &Path, exclude: &[&str]) -> SfumatoResult<Vec<PathBuf>>;
    /// Removes a regular file when present.
    fn remove_file(&self, path: &Path) -> SfumatoResult<()>;
    /// Atomically publishes one file into a destination directory.
    fn publish_atomic(&self, source: &Path, destination_dir: &Path) -> SfumatoResult<PathBuf>;
    /// Atomically replaces one published directory tree.
    fn publish_tree_atomic(&self, source: &Path, destination: &Path) -> SfumatoResult<PathBuf>;
    /// Removes a directory tree when present.
    fn remove_tree(&self, path: &Path) -> SfumatoResult<()>;
}
