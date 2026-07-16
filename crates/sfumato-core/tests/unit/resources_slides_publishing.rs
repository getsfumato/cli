use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use super::*;
use crate::filesystem::{TemporaryWorkspace, WorkspaceEntry};

#[derive(Default)]
struct PublicationWorkspace {
    published: Mutex<Vec<(PathBuf, PathBuf)>>,
    removed: Mutex<Vec<PathBuf>>,
    fail_publish: bool,
    fail_remove: bool,
}

impl WorkspaceFileSystem for PublicationWorkspace {
    fn publish_atomic(
        &self,
        source: &Path,
        destination: &Path,
    ) -> crate::errors::SfumatoResult<PathBuf> {
        if self.fail_publish {
            return Err(crate::errors::SfumatoError::artifact(
                crate::errors::ErrorClass::Permanent,
                "destination is read-only",
            ));
        }
        self.published
            .lock()
            .unwrap()
            .push((source.to_path_buf(), destination.to_path_buf()));
        Ok(destination.join(source.file_name().unwrap()))
    }

    fn remove_file(&self, path: &Path) -> crate::errors::SfumatoResult<()> {
        self.removed.lock().unwrap().push(path.to_path_buf());
        if self.fail_remove {
            return Err(crate::errors::SfumatoError::artifact(
                crate::errors::ErrorClass::Permanent,
                "permission denied",
            ));
        }
        Ok(())
    }

    fn temporary_directory(
        &self,
        _: &str,
    ) -> crate::errors::SfumatoResult<Box<dyn TemporaryWorkspace>> {
        unreachable!()
    }
    fn canonicalize(&self, _: &Path) -> crate::errors::SfumatoResult<PathBuf> {
        unreachable!()
    }
    fn read_text(&self, _: &Path) -> crate::errors::SfumatoResult<String> {
        unreachable!()
    }
    fn create_dir_all(&self, _: &Path) -> crate::errors::SfumatoResult<()> {
        unreachable!()
    }
    fn write(&self, _: &Path, _: &[u8]) -> crate::errors::SfumatoResult<()> {
        unreachable!()
    }
    fn copy_file(&self, _: &Path, _: &Path) -> crate::errors::SfumatoResult<()> {
        unreachable!()
    }
    fn is_file(&self, _: &Path) -> bool {
        unreachable!()
    }
    fn read_dir(&self, _: &Path) -> crate::errors::SfumatoResult<Vec<WorkspaceEntry>> {
        unreachable!()
    }
    fn copy_tree(&self, _: &Path, _: &Path, _: &[&str]) -> crate::errors::SfumatoResult<()> {
        unreachable!()
    }
    fn list_files(&self, _: &Path, _: &[&str]) -> crate::errors::SfumatoResult<Vec<PathBuf>> {
        unreachable!()
    }
}

#[test]
fn publication_failure_preserves_the_committed_result_as_a_warning() {
    let workspace = PublicationWorkspace {
        fail_publish: true,
        ..Default::default()
    };
    let result = publish_pdf(
        &workspace,
        Some(Path::new("/published")),
        Some(Path::new("/committed/deck.pdf")),
        Path::new("/staging/deck.pdf"),
    )
    .unwrap();

    assert!(result.path.is_none());
    assert!(result.warning.unwrap().contains("workspace revision"));
}

#[test]
fn missing_pdf_removes_the_stale_published_copy() {
    let workspace = PublicationWorkspace::default();
    let result = publish_pdf(
        &workspace,
        Some(Path::new("/published")),
        None,
        Path::new("/staging/deck.pdf"),
    )
    .unwrap();

    assert!(result.warning.is_none());
    assert_eq!(
        workspace.removed.lock().unwrap().as_slice(),
        [PathBuf::from("/published/deck.pdf")]
    );
}

#[test]
fn stale_pdf_removal_failure_is_non_fatal_and_reported() {
    let workspace = PublicationWorkspace {
        fail_remove: true,
        ..Default::default()
    };
    let result = publish_pdf(
        &workspace,
        Some(Path::new("/published")),
        None,
        Path::new("/staging/deck.pdf"),
    )
    .unwrap();

    assert!(result.path.is_none());
    assert!(result.warning.unwrap().contains("stale published PDF"));
}
