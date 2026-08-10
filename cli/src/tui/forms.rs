//! The generation and edit forms, and the CLI arguments they build.
//!
//! This is the largest part of the TUI's state and the part with the clearest
//! boundary: a form is fields plus the rules for turning them into the same
//! arguments the CLI parses, which is what keeps `ADR-0001`'s promise that both
//! frontends execute the same use cases. Holding it apart from the screen state
//! is also what would let an API expose the same field definitions to a web
//! frontend without dragging the terminal's navigation along with it.

use super::*;

#[derive(Clone, Debug)]
pub(super) enum FormField {
    Text {
        label: &'static str,
        value: String,
        placeholder: &'static str,
        multiline: bool,
    },
    Toggle {
        label: &'static str,
        value: bool,
    },
    Select {
        label: &'static str,
        options: Vec<String>,
        selected: usize,
    },
    MultiSelect {
        label: &'static str,
        options: Vec<String>,
        cursor: usize,
        selected: BTreeSet<usize>,
    },
    /// A value picked from a list the workspace already knows.
    ///
    /// Stores the identifier the CLI expects and names which list to offer, rather
    /// than carrying the list itself: the options are collected with the workspace
    /// snapshot, so a field built before that snapshot exists still knows what it
    /// wants. Free text could not tell the user what was available, and a typo only
    /// surfaced after the form was submitted.
    Choice {
        label: &'static str,
        value: String,
        placeholder: &'static str,
        source: ChoiceSource,
    },
    Submit {
        label: &'static str,
    },
}

/// Which list of workspace values a `Choice` field offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChoiceSource {
    Projects,
    Themes,
    SlideTemplates,
    PageTemplates,
    DocumentTemplates,
    TextModels,
    CodeModels,
    ImageModels,
    VideoModels,
    SpeechModels,
    ReviewerModels,
}

impl ChoiceSource {
    /// Resolves this source against a collected snapshot.
    pub(super) fn choices(self, options: &FormOptions) -> &[Choice] {
        match self {
            Self::Projects => &options.projects,
            Self::Themes => &options.themes,
            Self::SlideTemplates => &options.slide_templates,
            Self::PageTemplates => &options.page_templates,
            Self::DocumentTemplates => &options.document_templates,
            Self::TextModels => &options.text_models,
            Self::CodeModels => &options.code_models,
            Self::ImageModels => &options.image_models,
            Self::VideoModels => &options.video_models,
            Self::SpeechModels => &options.speech_models,
            Self::ReviewerModels => &options.reviewer_models,
        }
    }
}

impl FormField {
    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Text { label, .. }
            | Self::Choice { label, .. }
            | Self::Toggle { label, .. }
            | Self::Select { label, .. }
            | Self::MultiSelect { label, .. } => label,
            Self::Submit { label } => label,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct GenerateForm {
    pub(super) fields: Vec<FormField>,
    pub(super) field_ids: Vec<GenerateFieldId>,
    pub(super) selected: usize,
    pub(super) resource: GenerateResource,
    ui_options: Vec<String>,
    utility_plugins: Vec<String>,
    video_engine: VideoEngineArg,
    drafts: BTreeMap<GenerateResource, GenerateDraft>,
}

#[derive(Clone, Debug)]
struct GenerateDraft {
    fields: Vec<FormField>,
    field_ids: Vec<GenerateFieldId>,
    video_engine: VideoEngineArg,
}

struct CommonGenerationFields {
    instruction: String,
    inputs: Vec<PathBuf>,
    project: Option<String>,
    title: Option<String>,
    theme: Option<String>,
    model_overrides: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum GenerateResource {
    Slides,
    Page,
    Video,
    /// Appended rather than slotted beside slides: the selector's indices are what
    /// the reducer and the saved drafts key off, so a new resource goes last.
    Document,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum GenerateFieldId {
    Resource,
    Instruction,
    Sources,
    Project,
    Title,
    Theme,
    Template,
    Publish,
    TextModel,
    CodeModel,
    VideoModel,
    Reviewer,
    Ui,
    Plugins,
    ImageTool,
    VideoTool,
    AudioTool,
    /// Local plotting, which needs no model profile and so was easy to leave out
    /// of the form even though every other tool could be steered per run.
    ChartTool,
    /// Narration policy for Hyperframe, kept apart from the direct model's own
    /// audio switch so switching engines cannot carry one label onto the other.
    Narration,
    Voice,
    Engine,
    /// Creative direction for the film, which the TUI could not reach at all.
    Workflow,
    /// Websites captured as managed Hyperframe sources.
    Urls,
    /// Pause after contact-sheet review for human approval.
    VisualReview,
    /// Sheet to print a document on; the theme decides when left at its default.
    PageSize,
    /// Table-of-contents and cover-page overrides, both tri-state for the same
    /// reason the CLI pairs `--toc` with `--no-toc`: the theme owns the default,
    /// so a form has to be able to say nothing at all.
    TableOfContents,
    Cover,
    Duration,
    Resolution,
    AspectRatio,
    Fps,
    Quality,
    Audio,
    AllowCodeExecution,
    Review,
    DryRun,
    Submit,
}

#[derive(Clone, Debug)]
pub(super) struct EditForm {
    pub(super) fields: Vec<FormField>,
    pub(super) selected: usize,
}

impl Default for EditForm {
    fn default() -> Self {
        Self {
            fields: vec![
                FormField::Text {
                    label: "Deck",
                    value: String::new(),
                    placeholder: "~/.sfumato/Projects/university/slides/deck.md",
                    multiline: false,
                },
                FormField::Text {
                    label: "Instruction",
                    value: String::new(),
                    placeholder: "Clarify the explanation on slide four",
                    multiline: true,
                },
                FormField::Text {
                    label: "Project",
                    value: String::new(),
                    placeholder: "active project",
                    multiline: false,
                },
                FormField::Text {
                    label: "Text model",
                    value: String::new(),
                    placeholder: "project or user default",
                    multiline: false,
                },
                FormField::Submit {
                    label: "Edit slides",
                },
            ],
            selected: 0,
        }
    }
}

impl EditForm {
    pub(super) fn text(&self, label: &str) -> String {
        self.fields
            .iter()
            .find_map(|field| match field {
                FormField::Text {
                    label: field_label,
                    value,
                    ..
                }
                | FormField::Choice {
                    label: field_label,
                    value,
                    ..
                } if *field_label == label => Some(value.trim().to_string()),
                _ => None,
            })
            .unwrap_or_default()
    }

    pub(super) fn to_args(&self) -> Result<EditSlidesArgs> {
        let markdown_path = self.text("Deck");
        if markdown_path.is_empty() {
            anyhow::bail!("Deck path cannot be empty");
        }
        let instruction = self.text("Instruction");
        if instruction.is_empty() {
            anyhow::bail!("Instruction cannot be empty");
        }
        let optional = |value: String| (!value.is_empty()).then_some(value);
        let text_model = self.text("Text model");
        Ok(EditSlidesArgs {
            markdown_path: PathBuf::from(markdown_path),
            instruction,
            project: optional(self.text("Project")),
            model_overrides: if text_model.is_empty() {
                Vec::new()
            } else {
                vec![format!("text={text_model}")]
            },
            json: false,
        })
    }
}

impl Default for GenerateForm {
    fn default() -> Self {
        Self::with_plugins(Vec::new(), Vec::new())
    }
}

impl GenerateForm {
    pub(super) fn with_plugins(ui_options: Vec<String>, utility_plugins: Vec<String>) -> Self {
        let mut form = Self {
            fields: Vec::new(),
            field_ids: Vec::new(),
            selected: 0,
            resource: GenerateResource::Slides,
            ui_options,
            utility_plugins,
            video_engine: VideoEngineArg::Hyperframe,
            drafts: BTreeMap::new(),
        };
        form.rebuild(GenerateResource::Slides);
        form
    }

    pub(super) fn field_id(&self, index: usize) -> Option<GenerateFieldId> {
        self.field_ids.get(index).copied()
    }

    pub(super) fn switch_resource_from_selector(&mut self) {
        let selected = self.select_index(GenerateFieldId::Resource).unwrap_or(0);
        let resource = match selected {
            1 => GenerateResource::Page,
            2 => GenerateResource::Video,
            3 => GenerateResource::Document,
            _ => GenerateResource::Slides,
        };
        if resource != self.resource {
            self.drafts.insert(
                self.resource,
                GenerateDraft {
                    fields: self.fields.clone(),
                    field_ids: self.field_ids.clone(),
                    video_engine: self.video_engine,
                },
            );
            self.rebuild(resource);
        }
    }

    pub(super) fn switch_video_engine_from_selector(&mut self) {
        if self.resource != GenerateResource::Video {
            return;
        }
        let engine = match self.select_index(GenerateFieldId::Engine).unwrap_or(0) {
            1 => VideoEngineArg::Manim,
            2 => VideoEngineArg::Model,
            _ => VideoEngineArg::Hyperframe,
        };
        if engine == self.video_engine {
            return;
        }
        let existing = self
            .field_ids
            .iter()
            .copied()
            .zip(self.fields.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        self.video_engine = engine;
        let (mut fields, ids) = build_generation_fields(
            self.resource,
            &self.ui_options,
            &self.utility_plugins,
            self.video_engine,
        );
        for (field, id) in fields.iter_mut().zip(&ids) {
            if *id != GenerateFieldId::Engine
                && let Some(previous) = existing.get(id)
            {
                *field = previous.clone();
            }
        }
        self.fields = fields;
        self.field_ids = ids;
        self.selected = self
            .field_ids
            .iter()
            .position(|id| *id == GenerateFieldId::Engine)
            .unwrap_or(0);
    }

    fn rebuild(&mut self, resource: GenerateResource) {
        self.resource = resource;
        if let Some(draft) = self.drafts.remove(&resource) {
            self.fields = draft.fields;
            self.field_ids = draft.field_ids;
            self.video_engine = draft.video_engine;
            self.set_resource_selector(resource);
            self.selected = 0;
            return;
        }
        let (fields, ids) = build_generation_fields(
            resource,
            &self.ui_options,
            &self.utility_plugins,
            self.video_engine,
        );
        self.fields = fields;
        self.field_ids = ids;
        self.selected = 0;
    }

    fn set_resource_selector(&mut self, resource: GenerateResource) {
        if let Some(FormField::Select { selected, .. }) = self.fields.first_mut() {
            *selected = match resource {
                GenerateResource::Slides => 0,
                GenerateResource::Page => 1,
                GenerateResource::Video => 2,
                GenerateResource::Document => 3,
            };
        }
    }

    pub(super) fn text(&self, id: GenerateFieldId) -> String {
        self.field_ids
            .iter()
            .position(|candidate| *candidate == id)
            .and_then(|index| match &self.fields[index] {
                // A `Choice` holds the same kind of value a `Text` field held, so
                // every argument builder reads it without knowing the difference.
                FormField::Text { value, .. } | FormField::Choice { value, .. } => {
                    Some(value.trim().to_string())
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Which list the field at `id` offers, when it is a picker.
    pub(super) fn choice_source(&self, id: GenerateFieldId) -> Option<ChoiceSource> {
        self.field_ids
            .iter()
            .position(|candidate| *candidate == id)
            .and_then(|index| match &self.fields[index] {
                FormField::Choice { source, .. } => Some(*source),
                _ => None,
            })
    }

    /// Offers `folder` as the publish destination, without ever overwriting a typed one.
    ///
    /// Leaving publish empty is not the same as not caring where the resource goes:
    /// the artifact is committed to a managed revision under `~/.sfumato` either way,
    /// and nothing reaches the folder the sources came from. Filling the field in —
    /// visibly, while it is still editable — states that destination instead of
    /// deciding it silently at submit time. Only an empty field is filled, so a typed
    /// path is never clobbered.
    pub(super) fn offer_publish_folder(&mut self, folder: &str) {
        let Some(index) = self
            .field_ids
            .iter()
            .position(|candidate| *candidate == GenerateFieldId::Publish)
        else {
            return;
        };
        if let FormField::Text { value, .. } = &mut self.fields[index]
            && value.trim().is_empty()
        {
            *value = folder.to_string();
        }
    }

    /// Writes a picked value back, or clears it when `value` is empty.
    pub(super) fn set_choice(&mut self, id: GenerateFieldId, value: &str) {
        if let Some(index) = self.field_ids.iter().position(|candidate| *candidate == id)
            && let FormField::Choice { value: current, .. } = &mut self.fields[index]
        {
            *current = value.to_owned();
        }
    }

    /// Which list the currently focused field offers, when it is a picker.
    pub(super) fn focused_choice(&self) -> Option<GenerateFieldId> {
        let id = *self.field_ids.get(self.selected)?;
        self.choice_source(id).map(|_| id)
    }

    pub(super) fn toggle(&self, id: GenerateFieldId) -> bool {
        self.field_ids
            .iter()
            .position(|candidate| *candidate == id)
            .and_then(|index| match &self.fields[index] {
                FormField::Toggle { value, .. } => Some(*value),
                _ => None,
            })
            .unwrap_or(false)
    }

    fn select_index(&self, id: GenerateFieldId) -> Option<usize> {
        self.field_ids
            .iter()
            .position(|candidate| *candidate == id)
            .and_then(|index| match &self.fields[index] {
                FormField::Select { selected, .. } => Some(*selected),
                _ => None,
            })
    }

    fn selected_plugins(&self) -> Vec<String> {
        self.field_ids
            .iter()
            .position(|candidate| *candidate == GenerateFieldId::Plugins)
            .and_then(|index| match &self.fields[index] {
                FormField::MultiSelect {
                    options, selected, ..
                } => Some(
                    selected
                        .iter()
                        .filter_map(|index| options.get(*index).cloned())
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn common(&self) -> Result<CommonGenerationFields> {
        let instruction = self.text(GenerateFieldId::Instruction);
        if instruction.is_empty() {
            anyhow::bail!("Instruction cannot be empty");
        }
        let optional = |value: String| (!value.is_empty()).then_some(value);
        let inputs = split_values(&self.text(GenerateFieldId::Sources))
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let text = self.text(GenerateFieldId::TextModel);
        let models = if text.is_empty() {
            Vec::new()
        } else {
            vec![format!("text={text}")]
        };
        Ok(CommonGenerationFields {
            instruction,
            inputs,
            project: optional(self.text(GenerateFieldId::Project)),
            title: optional(self.text(GenerateFieldId::Title)),
            theme: optional(self.text(GenerateFieldId::Theme)),
            model_overrides: models,
        })
    }

    fn tool_flags(&self) -> (Vec<GenerationToolArg>, Vec<GenerationToolArg>) {
        let mut enabled = Vec::new();
        let mut disabled = Vec::new();
        for (id, tool) in [
            (GenerateFieldId::ImageTool, GenerationToolArg::ImageGen),
            (GenerateFieldId::VideoTool, GenerationToolArg::VideoGen),
            (GenerateFieldId::AudioTool, GenerationToolArg::AudioGen),
            (GenerateFieldId::ChartTool, GenerationToolArg::ChartGen),
        ] {
            match self.select_index(id) {
                Some(1) => enabled.push(tool),
                Some(2) => disabled.push(tool),
                _ => {}
            }
        }
        (enabled, disabled)
    }

    pub(super) fn to_slides_args(&self) -> Result<SlidesArgs> {
        let CommonGenerationFields {
            instruction,
            inputs,
            project,
            title,
            theme,
            model_overrides,
        } = self.common()?;
        let optional = |value: String| (!value.is_empty()).then_some(value);
        let (tools, disabled_tools) = self.tool_flags();
        Ok(SlidesArgs {
            inputs,
            instruction,
            title,
            template: optional(self.text(GenerateFieldId::Template)),
            out: optional(self.text(GenerateFieldId::Publish)).map(PathBuf::from),
            pdf: true,
            no_pdf: false,
            allow_code_execution: self.toggle(GenerateFieldId::AllowCodeExecution),
            dry_run: self.toggle(GenerateFieldId::DryRun),
            project,
            theme,
            model_overrides,
            review_model: optional(self.text(GenerateFieldId::Reviewer)),
            no_review: !self.toggle(GenerateFieldId::Review),
            json: false,
            tools,
            disabled_tools,
        })
    }

    pub(super) fn to_document_args(&self) -> Result<DocumentArgs> {
        let CommonGenerationFields {
            instruction,
            inputs,
            project,
            title,
            theme,
            model_overrides,
        } = self.common()?;
        let optional = |value: String| (!value.is_empty()).then_some(value);
        let (tools, disabled_tools) = self.tool_flags();
        // Each pair mirrors the CLI's `--flag` / `--no-flag`, so leaving the field on
        // its default sends neither and the theme keeps deciding.
        let (toc, no_toc) = self.theme_override(GenerateFieldId::TableOfContents);
        let (cover, no_cover) = self.theme_override(GenerateFieldId::Cover);
        Ok(DocumentArgs {
            inputs,
            instruction,
            title,
            template: optional(self.text(GenerateFieldId::Template)),
            out: optional(self.text(GenerateFieldId::Publish)).map(PathBuf::from),
            page_size: match self.select_index(GenerateFieldId::PageSize) {
                Some(1) => Some(DocumentPageSizeArg::A4),
                Some(2) => Some(DocumentPageSizeArg::Letter),
                _ => None,
            },
            toc,
            no_toc,
            cover,
            no_cover,
            allow_code_execution: self.toggle(GenerateFieldId::AllowCodeExecution),
            dry_run: self.toggle(GenerateFieldId::DryRun),
            project,
            theme,
            model_overrides,
            review_model: optional(self.text(GenerateFieldId::Reviewer)),
            no_review: !self.toggle(GenerateFieldId::Review),
            json: false,
            tools,
            disabled_tools,
        })
    }

    /// Reads a tri-state select as the CLI's on / off flag pair.
    fn theme_override(&self, id: GenerateFieldId) -> (bool, bool) {
        match self.select_index(id) {
            Some(1) => (true, false),
            Some(2) => (false, true),
            _ => (false, false),
        }
    }

    pub(super) fn to_page_args(&self) -> Result<PageArgs> {
        let CommonGenerationFields {
            instruction,
            inputs,
            project,
            title,
            theme,
            model_overrides,
        } = self.common()?;
        let optional = |value: String| (!value.is_empty()).then_some(value);
        let ui = match self.select_index(GenerateFieldId::Ui).unwrap_or(0) {
            0 => None,
            1 => Some("none".into()),
            index => self.ui_options.get(index - 2).cloned(),
        };
        let (tools, disabled_tools) = self.tool_flags();
        Ok(PageArgs {
            inputs,
            instruction,
            title,
            template: optional(self.text(GenerateFieldId::Template)),
            out: optional(self.text(GenerateFieldId::Publish)).map(PathBuf::from),
            allow_code_execution: self.toggle(GenerateFieldId::AllowCodeExecution),
            dry_run: self.toggle(GenerateFieldId::DryRun),
            project,
            theme,
            model_overrides,
            review_model: optional(self.text(GenerateFieldId::Reviewer)),
            plugins: self.selected_plugins(),
            disabled_plugins: Vec::new(),
            ui,
            shadcn: false,
            no_review: !self.toggle(GenerateFieldId::Review),
            json: false,
            tools,
            disabled_tools,
        })
    }

    pub(super) fn to_video_args(&self) -> Result<VideoArgs> {
        let CommonGenerationFields {
            instruction,
            inputs,
            project,
            title,
            theme,
            mut model_overrides,
        } = self.common()?;
        let optional = |value: String| (!value.is_empty()).then_some(value);
        for (capability, id) in [
            ("code", GenerateFieldId::CodeModel),
            ("video", GenerateFieldId::VideoModel),
        ] {
            let value = self.text(id);
            if !value.is_empty() {
                model_overrides.push(format!("{capability}={value}"));
            }
        }
        let engine = match self.select_index(GenerateFieldId::Engine).unwrap_or(0) {
            1 => VideoEngineArg::Manim,
            2 => VideoEngineArg::Model,
            _ => VideoEngineArg::Hyperframe,
        };
        let duration = self
            .text(GenerateFieldId::Duration)
            .parse::<u32>()
            .context("Duration must be a whole number of seconds")?;
        let fps = optional(self.text(GenerateFieldId::Fps))
            .map(|value| value.parse::<u32>().context("FPS must be a whole number"))
            .transpose()?;
        let audio = self
            .select_index(GenerateFieldId::Audio)
            .or_else(|| self.select_index(GenerateFieldId::Narration))
            .map(|index| match index {
                1 => VideoAudioArg::On,
                2 => VideoAudioArg::Off,
                _ => VideoAudioArg::Auto,
            });
        let (tools, disabled_tools) = self.tool_flags();
        let workflow = match self.select_index(GenerateFieldId::Workflow).unwrap_or(0) {
            1 => VideoWorkflowArg::Explainer,
            2 => VideoWorkflowArg::MotionGraphics,
            3 => VideoWorkflowArg::ProductLaunch,
            4 => VideoWorkflowArg::TalkingHead,
            5 => VideoWorkflowArg::Slideshow,
            6 => VideoWorkflowArg::General,
            _ => VideoWorkflowArg::Auto,
        };
        // Validated here rather than left to the workflow, so the form reports a bad
        // URL while the user is still looking at the field. Same rule the CLI applies.
        let urls = self
            .text(GenerateFieldId::Urls)
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                if value.starts_with("https://") || value.starts_with("http://") {
                    Ok(value.to_owned())
                } else {
                    Err(anyhow::anyhow!(
                        "Capture URL '{value}' must be an absolute http(s) URL"
                    ))
                }
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(VideoArgs {
            inputs,
            urls,
            instruction,
            title,
            engine,
            workflow,
            duration,
            out: optional(self.text(GenerateFieldId::Publish)).map(PathBuf::from),
            dry_run: self.toggle(GenerateFieldId::DryRun),
            project,
            theme,
            model_overrides,
            review_model: optional(self.text(GenerateFieldId::Reviewer)),
            no_review: !self.toggle(GenerateFieldId::Review),
            visual_review: self.toggle(GenerateFieldId::VisualReview),
            json: false,
            resolution: optional(self.text(GenerateFieldId::Resolution)),
            aspect_ratio: optional(self.text(GenerateFieldId::AspectRatio)),
            fps,
            quality: optional(self.text(GenerateFieldId::Quality)),
            audio,
            voice: optional(self.text(GenerateFieldId::Voice)),
            allow_code_execution: self.toggle(GenerateFieldId::AllowCodeExecution),
            tools,
            disabled_tools,
        })
    }
}

fn build_generation_fields(
    resource: GenerateResource,
    ui_plugins: &[String],
    utilities: &[String],
    video_engine: VideoEngineArg,
) -> (Vec<FormField>, Vec<GenerateFieldId>) {
    let mut pairs = vec![
        (
            GenerateFieldId::Resource,
            FormField::Select {
                label: "Resource",
                options: vec![
                    "Slides".into(),
                    "Page".into(),
                    "Video".into(),
                    "Document".into(),
                ],
                selected: match resource {
                    GenerateResource::Slides => 0,
                    GenerateResource::Page => 1,
                    GenerateResource::Video => 2,
                    GenerateResource::Document => 3,
                },
            },
        ),
        (
            GenerateFieldId::Instruction,
            FormField::Text {
                label: "Instruction",
                value: String::new(),
                placeholder: "Explain Fourier series visually",
                multiline: true,
            },
        ),
        (
            GenerateFieldId::Sources,
            text_generate_field("Sources", "notes, course-material"),
        ),
        (
            GenerateFieldId::Project,
            choice_field("Project", "active project", ChoiceSource::Projects),
        ),
        (
            GenerateFieldId::Title,
            text_generate_field("Title", "generated by the drafter"),
        ),
        (
            GenerateFieldId::Theme,
            choice_field("Theme", "project theme", ChoiceSource::Themes),
        ),
    ];
    if resource != GenerateResource::Video {
        pairs.push((
            GenerateFieldId::Template,
            // Filtered by resource: the layers below refuse a slides template used
            // for a page, so offering it would only produce a late failure.
            choice_field(
                "Template",
                "optional reusable structure",
                match resource {
                    GenerateResource::Page => ChoiceSource::PageTemplates,
                    GenerateResource::Document => ChoiceSource::DocumentTemplates,
                    _ => ChoiceSource::SlideTemplates,
                },
            ),
        ));
    }
    pairs.push((
        GenerateFieldId::Publish,
        text_generate_field(
            match resource {
                GenerateResource::Slides | GenerateResource::Document => "Publish PDF",
                GenerateResource::Page => "Publish page",
                GenerateResource::Video => "Publish MP4",
            },
            "optional folder",
        ),
    ));
    pairs.push((
        GenerateFieldId::TextModel,
        choice_field(
            "Text model",
            "project or user default",
            ChoiceSource::TextModels,
        ),
    ));
    if resource == GenerateResource::Video {
        match video_engine {
            VideoEngineArg::Hyperframe | VideoEngineArg::Manim => pairs.push((
                GenerateFieldId::CodeModel,
                choice_field(
                    "Code model",
                    "required by local engines",
                    ChoiceSource::CodeModels,
                ),
            )),
            VideoEngineArg::Model => pairs.push((
                GenerateFieldId::VideoModel,
                choice_field(
                    "Video model",
                    "required by model engine",
                    ChoiceSource::VideoModels,
                ),
            )),
        }
    }
    pairs.push((
        GenerateFieldId::Reviewer,
        choice_field(
            "Reviewer",
            "project or user reviewer",
            ChoiceSource::ReviewerModels,
        ),
    ));
    if resource == GenerateResource::Document {
        pairs.extend([
            (
                GenerateFieldId::PageSize,
                FormField::Select {
                    label: "Page size",
                    options: vec!["Theme default".into(), "A4".into(), "Letter".into()],
                    selected: 0,
                },
            ),
            (
                GenerateFieldId::TableOfContents,
                theme_override_select("Table of contents"),
            ),
            (GenerateFieldId::Cover, theme_override_select("Cover page")),
        ]);
    }
    if resource == GenerateResource::Page {
        let mut options = vec!["Project default".into(), "None".into()];
        options.extend(ui_plugins.iter().cloned());
        pairs.push((
            GenerateFieldId::Ui,
            FormField::Select {
                label: "UI library",
                options,
                selected: 0,
            },
        ));
        pairs.push((
            GenerateFieldId::Plugins,
            FormField::MultiSelect {
                label: "Utility plugins",
                options: utilities.to_vec(),
                cursor: 0,
                selected: BTreeSet::new(),
            },
        ));
    }
    if resource == GenerateResource::Video {
        pairs.extend([
            (
                GenerateFieldId::Engine,
                FormField::Select {
                    label: "Video engine",
                    options: vec!["Hyperframe".into(), "Manim".into(), "Model".into()],
                    selected: match video_engine {
                        VideoEngineArg::Hyperframe => 0,
                        VideoEngineArg::Manim => 1,
                        VideoEngineArg::Model => 2,
                    },
                },
            ),
            (
                GenerateFieldId::Workflow,
                FormField::Select {
                    label: "Workflow",
                    options: vec![
                        "Auto".into(),
                        "Explainer".into(),
                        "Motion graphics".into(),
                        "Product launch".into(),
                        "Talking head".into(),
                        "Slideshow".into(),
                        "General".into(),
                    ],
                    selected: 0,
                },
            ),
            (
                GenerateFieldId::Duration,
                text_generate_value("Duration (s)", "required", "15"),
            ),
            (
                GenerateFieldId::Resolution,
                text_generate_value("Resolution", "engine default", "1080p"),
            ),
            (
                GenerateFieldId::AspectRatio,
                text_generate_value("Aspect ratio", "16:9", "16:9"),
            ),
        ]);
        match video_engine {
            VideoEngineArg::Hyperframe => pairs.extend([
                (GenerateFieldId::Fps, text_generate_value("FPS", "30", "30")),
                (
                    GenerateFieldId::Quality,
                    text_generate_value("Quality", "high", "high"),
                ),
                (
                    GenerateFieldId::Narration,
                    FormField::Select {
                        label: "Narration",
                        options: vec!["Auto".into(), "On".into(), "Off".into()],
                        selected: 0,
                    },
                ),
                (
                    GenerateFieldId::Voice,
                    text_generate_field("Voice", "speech profile default"),
                ),
                (
                    GenerateFieldId::Urls,
                    text_generate_field("Capture URLs", "comma separated, https only"),
                ),
                (
                    GenerateFieldId::VisualReview,
                    FormField::Toggle {
                        label: "Visual review",
                        value: false,
                    },
                ),
            ]),
            VideoEngineArg::Manim => pairs.extend([
                (GenerateFieldId::Fps, text_generate_value("FPS", "30", "30")),
                (
                    GenerateFieldId::Quality,
                    text_generate_value("Quality", "high", "high"),
                ),
            ]),
            VideoEngineArg::Model => pairs.push((
                GenerateFieldId::Audio,
                FormField::Select {
                    label: "Audio",
                    options: vec!["Auto".into(), "On".into(), "Off".into()],
                    selected: 0,
                },
            )),
        }
    }
    // Every resource wires an image tool, so the guard this used to carry only
    // listed the resources that existed when it was written.
    pairs.push((GenerateFieldId::ImageTool, tool_select("Image generation")));
    if resource == GenerateResource::Page {
        pairs.push((GenerateFieldId::VideoTool, tool_select("Video generation")));
    }
    if matches!(resource, GenerateResource::Page | GenerateResource::Video) {
        pairs.push((GenerateFieldId::AudioTool, tool_select("Speech generation")));
    }
    // Every resource the drafter writes can plot, and charting needs no model
    // profile — so unlike the tools above it is offered unconditionally. Its real
    // gate is the consent toggle below.
    pairs.push((GenerateFieldId::ChartTool, tool_select("Chart generation")));
    // Raised out of the Manim branch, where it used to live: charting runs
    // generated Python for a deck or a page too, and those runs had no way to
    // grant the permission for one generation. Withheld from the direct model
    // engine alone, which runs no local code and whose command layer rejects the
    // flag outright, so offering it would only build a run that cannot start.
    if !(resource == GenerateResource::Video && video_engine == VideoEngineArg::Model) {
        pairs.push((
            GenerateFieldId::AllowCodeExecution,
            FormField::Toggle {
                // Short enough for the label column, which compacted the longer
                // wording to "Allow generate..." and lost the word that mattered.
                label: "Code execution",
                value: false,
            },
        ));
    }
    pairs.extend([
        (
            GenerateFieldId::Review,
            FormField::Toggle {
                label: "Review",
                value: true,
            },
        ),
        (
            GenerateFieldId::DryRun,
            FormField::Toggle {
                label: "Dry run",
                value: false,
            },
        ),
        (
            GenerateFieldId::Submit,
            FormField::Submit {
                label: match resource {
                    GenerateResource::Slides => "Generate slides",
                    GenerateResource::Page => "Generate page",
                    GenerateResource::Video => "Generate video",
                    GenerateResource::Document => "Generate document",
                },
            },
        ),
    ]);
    let (ids, fields) = pairs.into_iter().unzip();
    (fields, ids)
}

fn text_generate_field(label: &'static str, placeholder: &'static str) -> FormField {
    text_generate_value(label, placeholder, "")
}
fn text_generate_value(label: &'static str, placeholder: &'static str, value: &str) -> FormField {
    FormField::Text {
        label,
        value: value.into(),
        placeholder,
        multiline: false,
    }
}
/// Builds a field that picks from a workspace list.
fn choice_field(label: &'static str, placeholder: &'static str, source: ChoiceSource) -> FormField {
    FormField::Choice {
        label,
        value: String::new(),
        placeholder,
        source,
    }
}

fn tool_select(label: &'static str) -> FormField {
    FormField::Select {
        label,
        options: vec!["Project default".into(), "On".into(), "Off".into()],
        selected: 0,
    }
}

/// A tri-state the theme owns until the run says otherwise.
///
/// Two options would collapse the distinction the CLI keeps with its `--toc` /
/// `--no-toc` pairs: a form that can only say yes or no cannot leave the decision
/// where it belongs, and would silently override whatever the theme chose.
fn theme_override_select(label: &'static str) -> FormField {
    FormField::Select {
        label,
        options: vec!["Theme default".into(), "On".into(), "Off".into()],
        selected: 0,
    }
}
