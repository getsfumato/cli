//! Read-only workspace state the views render from.
//!
//! The views used to call `SfumatoApplication` directly while drawing, which made
//! every frame do filesystem work: the home screen alone read the project registry,
//! the model profiles, the connectors, and the themes, at the tick rate. Sitting
//! idle on that screen re-parsed four TOML documents twelve times a second, and the
//! answers cannot change without an action this process performed.
//!
//! Collecting them once and rendering from the result also decouples rendering from
//! the application facade. That is the shape a future HTTP API needs: this struct is
//! what an endpoint would serialize, and a frontend would consume the same fields
//! the TUI does.

use std::{path::PathBuf, sync::Arc};

use sfumato_core::{application::SfumatoApplication, config::Capability, templates::TemplateKind};

/// Everything the chrome and the home screen display about the workspace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct WorkspaceSnapshot {
    /// Active project, when one is selected.
    pub(super) project: Option<ProjectSummaryView>,
    /// Number of configured model profiles.
    pub(super) models: usize,
    /// Number of configured connectors.
    pub(super) connectors: usize,
    /// Number of installed themes.
    pub(super) themes: usize,
    /// Number of registered projects, so "no active project" can be distinguished
    /// from "no projects at all".
    pub(super) projects: usize,
    /// Values the form pickers offer.
    pub(super) options: FormOptions,
    /// Problems found while collecting, surfaced rather than swallowed.
    ///
    /// The previous code used `.ok()` and `unwrap_or(0)` at each call, so a broken
    /// registry rendered as a workspace with zero of everything.
    pub(super) problems: Vec<String>,
}

/// One selectable value: what to store, and what to show beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Choice {
    /// The identifier the CLI expects.
    pub(super) value: String,
    /// Context that makes the identifier recognisable — a connector, a path, a kind.
    pub(super) detail: String,
}

/// Everything the generation and edit forms can offer as a choice.
///
/// Collected with the rest of the snapshot, for the same reason: the form used to ask
/// for a project, a theme, a template, and two model profiles as free text, so filling
/// it required knowing every identifier by heart — and a typo surfaced as a failure
/// after the form was submitted rather than while it was being filled.
///
/// Filtered where filtering is what makes the list correct: a slides template must not
/// be offered to a document, and a text model must not be offered where a video model
/// is wanted, because the layers below reject both.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FormOptions {
    pub(super) projects: Vec<Choice>,
    pub(super) themes: Vec<Choice>,
    pub(super) slide_templates: Vec<Choice>,
    pub(super) page_templates: Vec<Choice>,
    pub(super) document_templates: Vec<Choice>,
    pub(super) text_models: Vec<Choice>,
    pub(super) code_models: Vec<Choice>,
    pub(super) image_models: Vec<Choice>,
    pub(super) video_models: Vec<Choice>,
    pub(super) speech_models: Vec<Choice>,
    pub(super) reviewer_models: Vec<Choice>,
}

/// The active project, reduced to what the views show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProjectSummaryView {
    pub(super) name: String,
    pub(super) path: PathBuf,
    /// Whether the registered path still holds a readable project.
    pub(super) available: bool,
    /// Theme the project renders with, which the next action will use.
    pub(super) theme: Option<String>,
}

impl WorkspaceSnapshot {
    /// Collects the workspace state once.
    ///
    /// Called on entering a screen that shows it and after an action that could
    /// change it — never from a draw.
    pub(super) fn collect(application: &Arc<SfumatoApplication>) -> Self {
        let mut snapshot = Self::default();
        match application.list_projects() {
            Ok(projects) => {
                snapshot.projects = projects.len();
                // A registered project whose directory is gone still appears, marked
                // unavailable, so the chrome can say so instead of showing a path
                // that is not there.
                snapshot.problems.extend(
                    projects
                        .iter()
                        .filter(|project| !project.available)
                        .map(|project| {
                            format!("project '{}' is missing its directory", project.name)
                        }),
                );
                snapshot.project =
                    projects
                        .into_iter()
                        .find(|project| project.active)
                        .map(|project| ProjectSummaryView {
                            // Read once, here: the header shows which theme the next
                            // action will use, and getting that wrong is a common way to
                            // be surprised by the output.
                            theme: application
                                .show_project(Some(&project.name))
                                .ok()
                                .map(|config| config.theme),
                            name: project.name,
                            path: project.path,
                            available: project.available,
                        });
            }
            Err(error) => snapshot.problems.push(format!("projects: {error}")),
        }
        snapshot.models = snapshot.count(application.list_models(), "models");
        snapshot.connectors = snapshot.count(application.list_connectors(), "connectors");
        snapshot.themes = snapshot.count(application.list_themes(), "themes");
        snapshot.options = snapshot.collect_options(application);
        snapshot
    }

    /// Gathers the values the form pickers offer.
    fn collect_options(&mut self, application: &Arc<SfumatoApplication>) -> FormOptions {
        let mut options = FormOptions::default();
        match application.list_projects() {
            Ok(projects) => {
                options.projects = projects
                    .into_iter()
                    .map(|project| Choice {
                        detail: if project.available {
                            project.path.display().to_string()
                        } else {
                            // Offered but labelled: hiding it would make a registered
                            // project look deleted instead of broken.
                            format!("missing — {}", project.path.display())
                        },
                        value: project.name,
                    })
                    .collect();
            }
            Err(error) => self.problems.push(format!("projects: {error}")),
        }
        match application.list_themes() {
            Ok(themes) => {
                options.themes = themes
                    .into_iter()
                    .map(|theme| Choice {
                        value: theme.name,
                        detail: String::new(),
                    })
                    .collect();
            }
            Err(error) => self.problems.push(format!("themes: {error}")),
        }
        match application.list_templates(None) {
            Ok(listing) => {
                for template in listing.entries {
                    let choice = Choice {
                        value: template.name,
                        detail: template.description,
                    };
                    match template.kind {
                        TemplateKind::Slides => options.slide_templates.push(choice),
                        TemplateKind::Page => options.page_templates.push(choice),
                        TemplateKind::Document => options.document_templates.push(choice),
                    }
                }
                self.problems.extend(
                    listing
                        .unreadable
                        .iter()
                        .map(|entry| format!("template '{}': {}", entry.name, entry.problem)),
                );
            }
            Err(error) => self.problems.push(format!("templates: {error}")),
        }
        match application.list_models() {
            Ok(models) => {
                for model in models {
                    let choice = Choice {
                        value: model.name.clone(),
                        detail: format!("{} · {}", model.connector, model.model),
                    };
                    // A profile appears under every capability it declares, because a
                    // profile that cannot do the job is refused further down.
                    if model.capabilities.contains(&Capability::Text) {
                        options.text_models.push(choice.clone());
                        options.reviewer_models.push(choice.clone());
                    }
                    if model.capabilities.contains(&Capability::Code) {
                        options.code_models.push(choice.clone());
                    }
                    if model.capabilities.contains(&Capability::Image) {
                        options.image_models.push(choice.clone());
                    }
                    if model.capabilities.contains(&Capability::Video) {
                        options.video_models.push(choice.clone());
                    }
                    if model.capabilities.contains(&Capability::Speech) {
                        options.speech_models.push(choice);
                    }
                }
            }
            Err(error) => self.problems.push(format!("models: {error}")),
        }
        options
    }

    fn count<T>(
        &mut self,
        result: sfumato_core::errors::SfumatoResult<Vec<T>>,
        what: &str,
    ) -> usize {
        match result {
            Ok(values) => values.len(),
            Err(error) => {
                self.problems.push(format!("{what}: {error}"));
                0
            }
        }
    }

    /// Name to display for the active project.
    pub(super) fn project_name(&self) -> &str {
        self.project
            .as_ref()
            .map(|project| project.name.as_str())
            .unwrap_or(if self.projects == 0 {
                "no project yet"
            } else {
                "no active project"
            })
    }

    /// One-line hint about what to do when there is no usable project.
    pub(super) fn project_hint(&self) -> Option<&'static str> {
        match &self.project {
            Some(project) if !project.available => {
                Some("its directory is missing — sfumato project remove <name>")
            }
            Some(_) => None,
            None if self.projects == 0 => Some("run Setup to create one"),
            None => Some("pick one in Projects"),
        }
    }
}
