//! Publication policy for processed slide artifacts.

use std::path::{Path, PathBuf};

use crate::{
    errors::{ResultContext as Context, SfumatoResult as Result},
    filesystem::WorkspaceFileSystem,
};

/// Result of publishing, or removing a stale copy of, a generated slide resource.
pub(super) struct SlidePublication {
    pub(super) pdf_path: Option<PathBuf>,
    pub(super) artifacts: Vec<PathBuf>,
    pub(super) warning: Option<String>,
}

/// Publishes a committed PDF with an Obsidian index without changing the committed revision.
pub(super) fn publish_slides(
    workspace: &dyn WorkspaceFileSystem,
    publish_root: Option<&Path>,
    committed_pdf: Option<&Path>,
    staged_pdf_path: &Path,
    title: &str,
    project: &str,
    revision: &str,
) -> Result<SlidePublication> {
    let Some(publish_root) = publish_root else {
        return Ok(SlidePublication {
            pdf_path: None,
            artifacts: Vec::new(),
            warning: None,
        });
    };
    let destination = slide_publish_destination(publish_root, staged_pdf_path)?;
    let legacy_pdf = publish_root.join(
        staged_pdf_path
            .file_name()
            .context("Generated PDF path must have a filename")?,
    );

    if let Some(pdf) = committed_pdf {
        return Ok(
            match publish_slide_tree(workspace, pdf, &destination, title, project, revision) {
                Ok((pdf_path, index_path)) => SlidePublication {
                    pdf_path: Some(pdf_path),
                    artifacts: vec![
                        index_path,
                        destination.join(
                            pdf.file_name()
                                .context("Generated PDF path must have a filename")?,
                        ),
                    ],
                    warning: workspace.remove_file(&legacy_pdf).err().map(|error| {
                        format!(
                            "Published the slide resource, but could not remove its legacy loose PDF: {error:#}"
                        )
                    }),
                },
                Err(error) => SlidePublication {
                    pdf_path: None,
                    artifacts: Vec::new(),
                    warning: Some(format!(
                        "Committed the workspace revision, but could not publish its slide resource: {error:#}"
                    )),
                },
            },
        );
    }

    let cleanup_errors = [
        workspace.remove_tree(&destination).err(),
        workspace.remove_file(&legacy_pdf).err(),
    ]
    .into_iter()
    .flatten()
    .map(|error| format!("{error:#}"))
    .collect::<Vec<_>>();
    let warning = (!cleanup_errors.is_empty()).then(|| {
        format!(
            "No PDF was generated and stale published slide output could not be removed: {}",
            cleanup_errors.join("; ")
        )
    });
    Ok(SlidePublication {
        pdf_path: None,
        artifacts: Vec::new(),
        warning,
    })
}

fn publish_slide_tree(
    workspace: &dyn WorkspaceFileSystem,
    pdf: &Path,
    destination: &Path,
    title: &str,
    project: &str,
    revision: &str,
) -> Result<(PathBuf, PathBuf)> {
    let temporary = workspace.temporary_directory("sfumato-slide-publish")?;
    let payload = temporary.path().join("payload");
    let filename = pdf
        .file_name()
        .context("Generated PDF path must have a filename")?;
    let staged_pdf = payload.join(filename);
    let staged_index = payload.join("index.md");
    workspace.create_dir_all(&payload)?;
    workspace.copy_file(pdf, &staged_pdf)?;
    workspace.write(
        &staged_index,
        obsidian_slide_index(title, project, revision, filename).as_bytes(),
    )?;
    workspace.publish_tree_atomic(&payload, destination)?;
    Ok((destination.join(filename), destination.join("index.md")))
}

fn slide_publish_destination(publish_root: &Path, pdf_path: &Path) -> Result<PathBuf> {
    let slug = pdf_path
        .file_stem()
        .context("Generated PDF path must have a filename stem")?;
    Ok(publish_root.join("_sfumato").join("slides").join(slug))
}

fn obsidian_slide_index(
    title: &str,
    project: &str,
    revision: &str,
    pdf_filename: &std::ffi::OsStr,
) -> String {
    let pdf_filename = pdf_filename.to_string_lossy();
    format!(
        "---\nsfumato: generated\nresource: slides\ntitle: {}\nproject: {}\nrevision: {}\n---\n\n# {title}\n\n> [!warning] Managed by Sfumato\n> Regenerate this resource through Sfumato instead of editing these files directly.\n\n[Open slide deck](./{pdf_filename})\n\n![[{pdf_filename}]]\n",
        serde_json::to_string(title).expect("strings always serialize"),
        serde_json::to_string(project).expect("strings always serialize"),
        serde_json::to_string(revision).expect("strings always serialize"),
    )
}

#[cfg(test)]
#[path = "../../../tests/unit/resources_slides_publishing.rs"]
mod tests;
