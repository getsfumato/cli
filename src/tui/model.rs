//! Runtime-neutral TUI state for one active resource operation.

use super::*;
use std::sync::Arc;

use sfumato_core::operation::{CancellationHandle, EventSink, OperationContext};

/// Owns UI job identity and the matching core cancellation handle.
pub(super) struct OperationLifecycle {
    next_job_id: u64,
    active_job_id: Option<u64>,
    cancellation: Option<CancellationHandle>,
}

impl Default for OperationLifecycle {
    fn default() -> Self {
        Self {
            next_job_id: 1,
            active_job_id: None,
            cancellation: None,
        }
    }
}

/// A running native connector read that the view can stop.
pub(super) struct ConnectorQuery {
    cancellation: CancellationHandle,
    task: JoinHandle<()>,
}

impl ConnectorQuery {
    pub(super) fn new(cancellation: CancellationHandle, task: JoinHandle<()>) -> Self {
        Self { cancellation, task }
    }

    /// Signals cancellation and drops the task without waiting on it.
    ///
    /// `Esc` must return control immediately; the task observes the token at its
    /// next checkpoint and its result is discarded because the view has moved on.
    pub(super) fn cancel(self) {
        self.cancellation.cancel();
        self.task.abort();
    }

    /// Signals cancellation and awaits the task, for shutdown.
    pub(super) async fn cancel_and_join(self) {
        self.cancellation.cancel();
        self.task.abort();
        let _ = self.task.await;
    }
}

impl OperationLifecycle {
    pub(super) fn next_job_id(&self) -> u64 {
        self.next_job_id
    }

    pub(super) fn begin(&mut self, events: Arc<dyn EventSink>) -> (u64, OperationContext) {
        self.cancel();
        let job_id = self.next_job_id;
        self.next_job_id = self.next_job_id.wrapping_add(1).max(1);
        let (cancellation, operation) = OperationContext::create(None, events);
        self.active_job_id = Some(job_id);
        self.cancellation = Some(cancellation);
        (job_id, operation)
    }

    pub(super) fn is_active(&self, job_id: u64) -> bool {
        self.active_job_id == Some(job_id)
    }

    pub(super) fn finish(&mut self, job_id: u64) -> bool {
        if !self.is_active(job_id) {
            return false;
        }
        self.active_job_id = None;
        self.cancellation = None;
        true
    }

    pub(super) fn cancel(&self) {
        if let Some(cancellation) = &self.cancellation {
            cancellation.cancel();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Section {
    Projects,
    Models,
    Connectors,
    Themes,
    Templates,
    Artifacts,
    Prompts,
    Configuration,
    Setup,
}

impl Section {
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Projects => "Projects",
            Self::Models => "Models",
            Self::Connectors => "Connectors",
            Self::Themes => "Themes",
            Self::Templates => "Templates",
            Self::Artifacts => "Artifacts",
            Self::Prompts => "Prompts",
            Self::Configuration => "Configuration",
            Self::Setup => "Setup",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Screen {
    Home,
    Browse(Section),
    Generate,
    Edit,
    Running,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ResourceOperation {
    Generate,
    GeneratePage,
    GenerateVideo,
    Edit,
}

#[derive(Clone, Debug)]
pub(super) struct BrowseRow {
    pub(super) title: String,
    pub(super) subtitle: String,
    pub(super) detail: String,
    pub(super) active: bool,
}

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
    Submit {
        label: &'static str,
    },
}

impl FormField {
    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Text { label, .. }
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
            };
        }
    }

    pub(super) fn text(&self, id: GenerateFieldId) -> String {
        self.field_ids
            .iter()
            .position(|candidate| *candidate == id)
            .and_then(|index| match &self.fields[index] {
                FormField::Text { value, .. } => Some(value.trim().to_string()),
                _ => None,
            })
            .unwrap_or_default()
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
                options: vec!["Slides".into(), "Page".into(), "Video".into()],
                selected: match resource {
                    GenerateResource::Slides => 0,
                    GenerateResource::Page => 1,
                    GenerateResource::Video => 2,
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
            text_generate_field("Project", "active project"),
        ),
        (
            GenerateFieldId::Title,
            text_generate_field("Title", "generated by the drafter"),
        ),
        (
            GenerateFieldId::Theme,
            text_generate_field("Theme", "project theme"),
        ),
    ];
    if resource != GenerateResource::Video {
        pairs.push((
            GenerateFieldId::Template,
            text_generate_field("Template", "optional reusable structure"),
        ));
    }
    pairs.push((
        GenerateFieldId::Publish,
        text_generate_field(
            match resource {
                GenerateResource::Slides => "Publish PDF",
                GenerateResource::Page => "Publish page",
                GenerateResource::Video => "Publish MP4",
            },
            "optional folder",
        ),
    ));
    pairs.push((
        GenerateFieldId::TextModel,
        text_generate_field("Text model", "project or user default"),
    ));
    if resource == GenerateResource::Video {
        match video_engine {
            VideoEngineArg::Hyperframe | VideoEngineArg::Manim => pairs.push((
                GenerateFieldId::CodeModel,
                text_generate_field("Code model", "required by local engines"),
            )),
            VideoEngineArg::Model => pairs.push((
                GenerateFieldId::VideoModel,
                text_generate_field("Video model", "required by model engine"),
            )),
        }
    }
    pairs.push((
        GenerateFieldId::Reviewer,
        text_generate_field("Reviewer", "project or user reviewer"),
    ));
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
                (
                    GenerateFieldId::AllowCodeExecution,
                    FormField::Toggle {
                        label: "Allow generated code execution",
                        value: false,
                    },
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
    if matches!(
        resource,
        GenerateResource::Slides | GenerateResource::Page | GenerateResource::Video
    ) {
        pairs.push((GenerateFieldId::ImageTool, tool_select("Image generation")));
    }
    if resource == GenerateResource::Page {
        pairs.push((GenerateFieldId::VideoTool, tool_select("Video generation")));
    }
    if matches!(resource, GenerateResource::Page | GenerateResource::Video) {
        pairs.push((GenerateFieldId::AudioTool, tool_select("Speech generation")));
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
fn tool_select(label: &'static str) -> FormField {
    FormField::Select {
        label,
        options: vec!["Project default".into(), "On".into(), "Off".into()],
        selected: 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ActivityKind {
    Stage,
    Model,
    ToolCall,
    ToolResult,
    Warning,
    Success,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BrowseFocus {
    Actions,
    Rows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BrowseAction {
    ProjectCreate,
    ProjectActivate,
    ProjectRemove,
    ModelAdd,
    ModelEdit,
    ModelUse,
    ModelRemove,
    ConnectorSetup,
    ConnectorModels,
    ConnectorStatus,
    ThemeCreate,
    ThemeImport,
    ThemeExport,
    ThemeUse,
    TemplateCreate,
    ArtifactAdd,
    ArtifactRemove,
    PromptCustomizeUser,
    PromptCustomizeProject,
    PromptValidate,
    ConfigSet,
    ConfigDelete,
    SetupUser,
}

impl BrowseAction {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::ProjectCreate => "Create",
            Self::ProjectActivate => "Activate",
            Self::ProjectRemove => "Remove",
            Self::ModelAdd => "Add",
            Self::ModelEdit => "Edit",
            Self::ModelUse => "Set default",
            Self::ModelRemove => "Remove",
            Self::ConnectorSetup => "Setup",
            Self::ConnectorModels => "Model catalog",
            Self::ConnectorStatus => "Native status",
            Self::ThemeCreate => "Create",
            Self::ThemeImport => "Import DESIGN.md",
            Self::ThemeExport => "Export DESIGN.md",
            Self::ThemeUse => "Apply",
            Self::TemplateCreate => "Create",
            Self::ArtifactAdd => "Add",
            Self::ArtifactRemove => "Remove",
            Self::PromptCustomizeUser => "Customize user",
            Self::PromptCustomizeProject => "Customize project",
            Self::PromptValidate => "Validate",
            Self::ConfigSet => "Set value",
            Self::ConfigDelete => "Delete value",
            Self::SetupUser => "Initialize user",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum OperationKind {
    ProjectCreate,
    ProjectRemove,
    ModelAdd,
    ModelEdit,
    ModelUse,
    ModelRemove,
    ConnectorSetup,
    ThemeCreate,
    ThemeImport,
    ThemeExport,
    ThemeUse,
    TemplateCreate,
    ArtifactAdd,
    ArtifactRemove,
    PromptCustomize(PromptOverrideScope),
    PromptValidate,
    ConfigSet,
    ConfigDelete,
    SetupUser,
}

/// Label of the optional credential field on the connector setup form.
///
/// Shared so the form definition and the preset-dependency pass cannot drift.
pub(super) const API_KEY_ENV_FIELD: &str = "API key environment";

#[derive(Clone, Debug)]
pub(super) struct OperationForm {
    pub(super) title: &'static str,
    pub(super) kind: OperationKind,
    pub(super) target: Option<String>,
    pub(super) fields: Vec<FormField>,
    pub(super) selected: usize,
}

impl OperationForm {
    pub(super) fn text(&self, label: &str) -> String {
        self.fields
            .iter()
            .find_map(|field| match field {
                FormField::Text {
                    label: field_label,
                    value,
                    ..
                } if *field_label == label => Some(value.trim().to_string()),
                _ => None,
            })
            .unwrap_or_default()
    }

    pub(super) fn toggle(&self, label: &str) -> bool {
        self.fields
            .iter()
            .find_map(|field| match field {
                FormField::Toggle {
                    label: field_label,
                    value,
                } if *field_label == label => Some(*value),
                _ => None,
            })
            .unwrap_or(false)
    }

    /// Re-derives the fields that depend on a select choice.
    ///
    /// Called after every select move so the form never offers a field the
    /// chosen preset rejects, and never keeps another preset's defaults.
    pub(super) fn apply_select_dependencies(&mut self) {
        match self.kind {
            OperationKind::ConnectorSetup => {
                let accepts = ConnectorPreset::from_str(&self.select("Preset"))
                    .map(ConnectorPreset::accepts_api_key_env)
                    // An unparsable value is rejected on submit; keep the field.
                    .unwrap_or(true);
                self.set_field_present(API_KEY_ENV_FIELD, accepts, || FormField::Text {
                    label: API_KEY_ENV_FIELD,
                    value: String::new(),
                    placeholder: "optional CI environment variable",
                    multiline: false,
                });
            }
            OperationKind::SetupUser => {
                let Ok(preset) = ConnectorPreset::from_str(&self.select("Connector")) else {
                    return;
                };
                // Overwritten rather than merged: these two fields are the chosen
                // preset's defaults, and a stale Ollama profile name or model id
                // would be written into the config as-is.
                self.set_text("Profile", preset.default_profile_name());
                self.set_text("Model ID", preset.default_model());
            }
            _ => {}
        }
    }

    fn set_text(&mut self, label: &str, value: &str) {
        for field in &mut self.fields {
            if let FormField::Text {
                label: field_label,
                value: field_value,
                ..
            } = field
                && *field_label == label
            {
                *field_value = value.to_string();
            }
        }
    }

    fn set_field_present(
        &mut self,
        label: &'static str,
        present: bool,
        build: impl FnOnce() -> FormField,
    ) {
        let position = self.fields.iter().position(|field| field.label() == label);
        match (present, position) {
            (true, None) => {
                let insert_at = self
                    .fields
                    .iter()
                    .position(|field| matches!(field, FormField::Submit { .. }))
                    .unwrap_or(self.fields.len());
                self.fields.insert(insert_at, build());
                if self.selected >= insert_at {
                    self.selected += 1;
                }
            }
            (false, Some(index)) => {
                self.fields.remove(index);
                if self.selected > index {
                    self.selected -= 1;
                }
                self.selected = self.selected.min(self.fields.len().saturating_sub(1));
            }
            _ => {}
        }
    }

    pub(super) fn select(&self, label: &str) -> String {
        self.fields
            .iter()
            .find_map(|field| match field {
                FormField::Select {
                    label: field_label,
                    options,
                    selected,
                } if *field_label == label => options.get(*selected).cloned(),
                _ => None,
            })
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub(super) struct Activity {
    pub(super) kind: ActivityKind,
    pub(super) title: String,
    pub(super) detail: String,
    pub(super) image_path: Option<PathBuf>,
}

impl Activity {
    pub(super) fn from_event(event: &TextGenerationEvent) -> Self {
        match event {
            TextGenerationEvent::StageStarted { stage, profile } => Self {
                kind: ActivityKind::Stage,
                title: stage.as_str().to_string(),
                detail: profile
                    .as_ref()
                    .map(|profile| format!("model profile: {profile}"))
                    .unwrap_or_default(),
                image_path: None,
            },
            TextGenerationEvent::RequestStarted { round } => Self {
                kind: ActivityKind::Model,
                title: format!("Model request {round}"),
                detail: "Waiting for a response".to_string(),
                image_path: None,
            },
            TextGenerationEvent::ModelSelected {
                model,
                display_name,
            } => Self {
                kind: ActivityKind::Model,
                title: format!("Selected {display_name}"),
                detail: model.clone(),
                image_path: None,
            },
            TextGenerationEvent::ToolCallRequested { name, arguments } => Self {
                kind: ActivityKind::ToolCall,
                title: tool_name(name),
                detail: format_tool_arguments(arguments),
                image_path: None,
            },
            TextGenerationEvent::ToolCallSucceeded { name, result } => {
                let (detail, image_path) = format_tool_result(name, result);
                Self {
                    kind: ActivityKind::ToolResult,
                    title: format!("{} complete", tool_name(name)),
                    detail,
                    image_path,
                }
            }
            TextGenerationEvent::ToolCallFailed { name, error } => Self {
                kind: ActivityKind::Warning,
                title: format!("{} failed", tool_name(name)),
                detail: compact(error, 300),
                image_path: None,
            },
            TextGenerationEvent::ResponseCompleted => Self {
                kind: ActivityKind::Success,
                title: "Model response complete".to_string(),
                detail: String::new(),
                image_path: None,
            },
            TextGenerationEvent::DraftTitleRepairStarted { error } => Self {
                kind: ActivityKind::Warning,
                title: "Repairing deck title".to_string(),
                detail: compact(error, 300),
                image_path: None,
            },
            TextGenerationEvent::ReviewRetryStarted { attempt, error } => Self {
                kind: ActivityKind::Warning,
                title: format!("Content review retry {attempt}"),
                detail: compact(error, 300),
                image_path: None,
            },
            TextGenerationEvent::ContextCompactionStarted {
                stage,
                original_chars,
                compacted_chars,
            } => Self {
                kind: ActivityKind::Warning,
                title: "Compacting model context".to_string(),
                detail: format!(
                    "{} reduced from {original_chars} to {compacted_chars} characters",
                    stage.as_str()
                ),
                image_path: None,
            },
            TextGenerationEvent::LayoutCheckCompleted { issues } => Self {
                kind: if *issues == 0 {
                    ActivityKind::Success
                } else {
                    ActivityKind::Warning
                },
                title: if *issues == 0 {
                    "Layout check passed".to_string()
                } else {
                    format!("{issues} slide(s) need repair")
                },
                detail: String::new(),
                image_path: None,
            },
            TextGenerationEvent::LayoutSlideRepairStarted {
                slide,
                position,
                total,
                profile,
            } => Self {
                kind: ActivityKind::ToolCall,
                title: format!("Repairing slide {slide}"),
                detail: format!("{position} of {total} with {profile}"),
                image_path: None,
            },
            TextGenerationEvent::LayoutSlideRepairRetryStarted {
                slide,
                attempt,
                error,
            } => Self {
                kind: ActivityKind::Warning,
                title: format!("Slide {slide} repair retry {attempt}"),
                detail: compact(error, 300),
                image_path: None,
            },
            TextGenerationEvent::SourceRepairStarted { reason, scene } => Self {
                kind: ActivityKind::Warning,
                title: match scene {
                    Some(scene) => format!("Repairing {scene}"),
                    None => "Repairing source".to_string(),
                },
                detail: compact(reason, 300),
                image_path: None,
            },
        }
    }

    pub(super) fn from_operation_event(event: &OperationEvent) -> Option<Self> {
        if event.kind == OperationEventKind::Progress {
            return None;
        }
        let title = operation_stage_label(event.stage).to_string();
        let detail = event
            .fields
            .iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect::<Vec<_>>()
            .join(", ");
        Some(Self {
            kind: match event.kind {
                OperationEventKind::Completed => ActivityKind::Success,
                OperationEventKind::Warning | OperationEventKind::Retry => ActivityKind::Warning,
                OperationEventKind::Started | OperationEventKind::Progress => ActivityKind::Model,
                _ => ActivityKind::Model,
            },
            title,
            detail,
            image_path: None,
        })
    }
}

pub(super) fn operation_stage_label(stage: OperationStage) -> &'static str {
    match stage {
        OperationStage::Resolve => "Resolving configuration",
        OperationStage::ReadSources => "Reading source material",
        OperationStage::RenderPrompt => "Rendering prompts",
        OperationStage::Draft => "Drafting resource",
        OperationStage::Edit => "Editing resource",
        OperationStage::Review => "Reviewing content",
        OperationStage::InspectLayout => "Inspecting layout",
        OperationStage::Repair => "Repairing resource",
        OperationStage::Render => "Rendering artifacts",
        OperationStage::CommitArtifacts => "Committing revision",
        OperationStage::Publish => "Publishing output",
        // `OperationStage` is `#[non_exhaustive]`, so a downstream match cannot be
        // exhaustive and a new variant cannot produce a compile error here — unlike
        // `stage_label` in `view.rs`, which covers `GenerationStage` from this crate.
        // Falling back to the stage's own stable name means an unlabelled stage still
        // says which one it is, instead of every one of them reading as the same
        // opaque "Running operation" in the activity list.
        other => other.as_str(),
    }
}

pub(super) enum UiMessage {
    GenerationEvent {
        job_id: u64,
        event: TextGenerationEvent,
    },
    OperationEvent {
        job_id: u64,
        event: OperationEvent,
    },
    ResourceFinished {
        job_id: u64,
        result: Box<Result<ResourceResult, String>>,
    },
    ResourceCancelled {
        job_id: u64,
    },
    ConnectorQueryFinished {
        connector: String,
        result: Result<Vec<BrowseRow>, String>,
    },
}

pub(super) enum ResourceResult {
    Generated(GenerateSlidesResult),
    GeneratedPage(GeneratePageResult),
    GeneratedVideo(GenerateVideoResult),
    Edited(EditSlidesResult),
}

impl ResourceResult {
    pub(super) fn markdown_path(&self) -> &std::path::Path {
        match self {
            Self::Generated(result) => &result.markdown_path,
            Self::GeneratedPage(result) => &result.html_path,
            Self::GeneratedVideo(result) => &result.video_path,
            Self::Edited(result) => &result.markdown_path,
        }
    }

    pub(super) fn warnings(&self) -> &[String] {
        match self {
            Self::Generated(result) => &result.warnings,
            Self::GeneratedPage(result) => &result.warnings,
            Self::GeneratedVideo(result) => &result.output.warnings,
            Self::Edited(result) => &result.warnings,
        }
    }

    pub(super) fn completion_message(&self) -> &'static str {
        match self {
            Self::Generated(_) => "Generation complete",
            Self::GeneratedPage(_) => "Page generation complete",
            Self::GeneratedVideo(_) => "Video generation complete",
            Self::Edited(_) => "Slide edit complete",
        }
    }
}

pub(super) struct App {
    pub(super) application: Arc<SfumatoApplication>,
    pub(super) screen: Screen,
    pub(super) nav_index: usize,
    pub(super) browse_rows: Vec<BrowseRow>,
    pub(super) browse_index: usize,
    pub(super) browse_focus: BrowseFocus,
    pub(super) browse_action_index: usize,
    pub(super) browse_detail_scroll: u16,
    pub(super) connector_query_source: Option<String>,
    pub(super) operation: Option<OperationForm>,
    pub(super) form: GenerateForm,
    pub(super) edit_form: EditForm,
    pub(super) resource_operation: ResourceOperation,
    pub(super) activities: Vec<Activity>,
    pub(super) activity_index: usize,
    pub(super) current_stage: Option<GenerationStage>,
    pub(super) generation_failed: bool,
    pub(super) result: Option<ResourceResult>,
    pub(super) status: Option<(String, bool)>,
    pub(super) tick: usize,
    pub(super) should_quit: bool,
    pub(super) sender: Sender<UiMessage>,
    pub(super) messages: Receiver<UiMessage>,
    pub(super) jobs: OperationLifecycle,
    pub(super) active_task: Option<JoinHandle<()>>,
    /// In-flight native connector read, kept so it can be cancelled.
    ///
    /// Separate from `jobs`, which owns the generation job: browsing a connector
    /// must not cancel a running generation, and `Esc` in the browse view must
    /// not have to wait on one.
    pub(super) connector_query: Option<ConnectorQuery>,
    /// Workspace state the chrome and home screen render from.
    ///
    /// Refreshed by `refresh_snapshot`, never during a draw: the values only change
    /// when this process performs an action, and reading them per frame made the
    /// render loop do filesystem work.
    pub(super) snapshot: WorkspaceSnapshot,
    /// When the running operation started, for the elapsed clock.
    ///
    /// A long video render can run for minutes; without a clock there is no way to
    /// tell a slow stage from a stuck one.
    pub(super) started_at: Option<std::time::Instant>,
    pub(super) picker: Picker,
    pub(super) image: Option<StatefulProtocol>,
    pub(super) effects: EffectManager<&'static str>,
    pub(super) dirty: bool,
}

#[cfg(test)]
#[path = "../../tests/unit/tui_model.rs"]
mod tests;
