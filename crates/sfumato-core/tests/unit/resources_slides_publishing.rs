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

struct PublicationTemporary(PathBuf);

impl TemporaryWorkspace for PublicationTemporary {
    fn path(&self) -> &Path {
        &self.0
    }
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
        Ok(Box::new(PublicationTemporary(PathBuf::from(
            "/temporary/sfumato-slide-publish",
        ))))
    }
    fn canonicalize(&self, _: &Path) -> crate::errors::SfumatoResult<PathBuf> {
        unreachable!()
    }
    fn read_text(&self, _: &Path) -> crate::errors::SfumatoResult<String> {
        unreachable!()
    }
    fn read_bytes(&self, _: &Path) -> crate::errors::SfumatoResult<Vec<u8>> {
        unreachable!()
    }
    fn create_dir_all(&self, _: &Path) -> crate::errors::SfumatoResult<()> {
        Ok(())
    }
    fn write(&self, _: &Path, _: &[u8]) -> crate::errors::SfumatoResult<()> {
        Ok(())
    }
    fn copy_file(&self, _: &Path, _: &Path) -> crate::errors::SfumatoResult<()> {
        Ok(())
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
    fn publish_tree_atomic(
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
        Ok(destination.to_path_buf())
    }
    fn remove_tree(&self, path: &Path) -> crate::errors::SfumatoResult<()> {
        self.remove_file(path)
    }
}

#[test]
fn publication_failure_preserves_the_committed_result_as_a_warning() {
    let workspace = PublicationWorkspace {
        fail_publish: true,
        ..Default::default()
    };
    let result = publish_slides(
        &workspace,
        Some(Path::new("/published")),
        Some(Path::new("/committed/deck.pdf")),
        Path::new("/staging/deck.pdf"),
        "Deck",
        "university",
        "rev-1",
    )
    .unwrap();

    assert!(result.pdf_path.is_none());
    assert!(result.warning.unwrap().contains("workspace revision"));
}

#[test]
fn missing_pdf_removes_the_stale_published_copy() {
    let workspace = PublicationWorkspace::default();
    let result = publish_slides(
        &workspace,
        Some(Path::new("/published")),
        None,
        Path::new("/staging/deck.pdf"),
        "Deck",
        "university",
        "rev-1",
    )
    .unwrap();

    assert!(result.warning.is_none());
    assert_eq!(
        workspace.removed.lock().unwrap().as_slice(),
        [
            PathBuf::from("/published/_sfumato/slides/deck"),
            PathBuf::from("/published/deck.pdf"),
        ]
    );
}

#[test]
fn stale_pdf_removal_failure_is_non_fatal_and_reported() {
    let workspace = PublicationWorkspace {
        fail_remove: true,
        ..Default::default()
    };
    let result = publish_slides(
        &workspace,
        Some(Path::new("/published")),
        None,
        Path::new("/staging/deck.pdf"),
        "Deck",
        "university",
        "rev-1",
    )
    .unwrap();

    assert!(result.pdf_path.is_none());
    assert!(result.warning.unwrap().contains("stale published slide"));
}

#[test]
fn publishes_a_slide_resource_under_the_visible_sfumato_namespace() {
    let workspace = PublicationWorkspace::default();
    let result = publish_slides(
        &workspace,
        Some(Path::new("/vault")),
        Some(Path::new("/committed/fourier-series.pdf")),
        Path::new("/staging/fourier-series.pdf"),
        "Fourier Series",
        "university",
        "rev-2",
    )
    .unwrap();

    assert_eq!(
        result.pdf_path,
        Some(PathBuf::from(
            "/vault/_sfumato/slides/fourier-series/fourier-series.pdf"
        ))
    );
    assert_eq!(
        result.artifacts,
        [
            PathBuf::from("/vault/_sfumato/slides/fourier-series/index.md"),
            PathBuf::from("/vault/_sfumato/slides/fourier-series/fourier-series.pdf"),
        ]
    );
    assert_eq!(
        workspace.published.lock().unwrap()[0].1,
        PathBuf::from("/vault/_sfumato/slides/fourier-series")
    );
    assert_eq!(
        workspace.removed.lock().unwrap().as_slice(),
        [PathBuf::from("/vault/fourier-series.pdf")]
    );
}

#[test]
fn slide_index_links_and_embeds_the_pdf_for_obsidian() {
    let index = obsidian_slide_index(
        "Fourier Series",
        "university",
        "rev-2",
        std::ffi::OsStr::new("fourier-series.pdf"),
    );

    assert!(index.contains("resource: slides"));
    assert!(index.contains("[Open slide deck](./fourier-series.pdf)"));
    assert!(index.contains("![[fourier-series.pdf]]"));
}
