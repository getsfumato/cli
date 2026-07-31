use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "sfumato")]
#[command(about = "Generate Obsidian-friendly learning resources with local or cloud models.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Init {
        #[command(subcommand)]
        target: InitTarget,
    },
    #[command(about = "Show and edit Sfumato configuration")]
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },
    Connector {
        #[command(subcommand)]
        command: ConnectorCommands,
    },
    Model {
        #[command(subcommand)]
        command: ModelCommands,
    },
    Theme {
        #[command(subcommand)]
        command: ThemeCommands,
    },
    #[command(about = "Manage reusable structural generation templates")]
    Template {
        #[command(subcommand)]
        command: TemplateCommands,
    },
    #[command(about = "Manage reusable project artifacts such as logos and icons")]
    Artifact {
        #[command(subcommand)]
        command: ArtifactCommands,
    },
    #[command(about = "Inspect and customize model prompt templates")]
    Prompt {
        #[command(subcommand)]
        command: PromptCommands,
    },
    #[command(about = "Install and configure offline page plugins")]
    Plugin {
        #[command(subcommand)]
        command: PluginCommands,
    },
    #[command(about = "Configure optional model-facing generation tools")]
    Tool {
        #[command(subcommand)]
        command: ToolCommands,
    },
    #[command(about = "Install and diagnose local renderers")]
    Renderer {
        #[command(subcommand)]
        command: RendererCommands,
    },
    #[command(about = "Preview or approve a paused Hyperframe video review")]
    Video {
        #[command(subcommand)]
        command: VideoCommands,
    },
    Generate {
        #[command(subcommand)]
        command: GenerateCommands,
    },
    #[command(about = "Edit existing generated resources without regenerating them")]
    Edit {
        #[command(subcommand)]
        command: EditCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum InitTarget {
    User {
        #[arg(long, help = "Use default values without asking setup questions")]
        yes: bool,

        #[arg(long, help = "Overwrite an existing user config")]
        force: bool,
    },
    Project(InitProjectArgs),
}

#[derive(Debug, Subcommand)]
pub enum GenerateCommands {
    Slides(SlidesArgs),
    #[command(visible_aliases = ["doc", "docs"])]
    Document(DocumentArgs),
    #[command(visible_alias = "pages")]
    Page(PageArgs),
    Video(VideoArgs),
}

#[derive(Debug, Subcommand)]
pub enum VideoCommands {
    Preview(VideoReviewArgs),
    Approve(VideoApproveArgs),
}

#[derive(Debug, Args)]
pub struct VideoReviewArgs {
    pub review_id: String,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct VideoApproveArgs {
    pub review_id: String,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(
        long,
        help = "Override the saved destination and publish under <folder>/_sfumato/videos"
    )]
    pub out: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum ToolCommands {
    List(PluginProjectArgs),
    Enable(ToolProjectArgs),
    Disable(ToolProjectArgs),
}

#[derive(Debug, Args)]
pub struct ToolProjectArgs {
    #[arg(value_enum)]
    pub tool: GenerationToolArg,
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
// Spellings mirror `GenerationToolKind`, which is the configuration and CLI
// vocabulary; renaming them to please the lint would rename the flags.
#[allow(clippy::enum_variant_names)]
pub enum GenerationToolArg {
    ImageGen,
    VideoGen,
    AudioGen,
}

#[derive(Debug, Subcommand)]
pub enum RendererCommands {
    List,
    Install(RendererNameArgs),
    Remove(RendererNameArgs),
    Doctor(RendererDoctorArgs),
}

#[derive(Debug, Args)]
pub struct RendererNameArgs {
    #[arg(value_enum)]
    pub renderer: LocalVideoRendererArg,
}

#[derive(Debug, Args)]
pub struct RendererDoctorArgs {
    #[arg(value_enum)]
    pub renderer: Option<LocalVideoRendererArg>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum LocalVideoRendererArg {
    Hyperframe,
    Manim,
    #[value(name = "pagedjs")]
    PagedJs,
}

#[derive(Debug, Subcommand)]
pub enum PluginCommands {
    #[command(about = "List all supported page plugins")]
    List(PluginProjectArgs),
    #[command(about = "Show metadata and model guidance for a page plugin")]
    Show(PluginNameArgs),
    #[command(about = "Download and install a page plugin")]
    Install(PluginInstallArgs),
    #[command(about = "Update an installed page plugin")]
    Update(PluginNameArgs),
    #[command(about = "Enable an installed plugin for a project")]
    Enable(PluginProjectNameArgs),
    #[command(about = "Disable a plugin for a project")]
    Disable(PluginProjectNameArgs),
}

#[derive(Debug, Args)]
pub struct PluginProjectArgs {
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct PluginInstallArgs {
    pub id: String,
    #[arg(long)]
    pub version: Option<String>,
}

#[derive(Debug, Args)]
pub struct PluginProjectNameArgs {
    pub id: String,
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum TemplateCommands {
    #[command(about = "Create a reusable template package")]
    Create(TemplateCreateArgs),
    #[command(about = "List installed reusable templates")]
    List(TemplateListArgs),
    #[command(about = "Show a reusable template and its structural source")]
    Show(TemplateShowArgs),
}

#[derive(Debug, Subcommand)]
pub enum ArtifactCommands {
    #[command(about = "Copy and register a reusable project artifact")]
    Add(ArtifactAddArgs),
    #[command(about = "Edit artifact metadata or reassign a themed variant")]
    Edit(ArtifactEditArgs),
    #[command(about = "List reusable artifacts for a project")]
    List(ArtifactProjectArgs),
    #[command(about = "Show one reusable project artifact")]
    Show(ArtifactNameArgs),
    #[command(about = "Remove a reusable artifact without touching its original source")]
    Remove(ArtifactNameArgs),
}

#[derive(Debug, Args)]
pub struct ArtifactAddArgs {
    pub path: PathBuf,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long, help = "Accessible description used when embedding the artifact")]
    pub alt_text: Option<String>,
    #[arg(long = "tag", help = "Repeatable semantic metadata tag")]
    pub tags: Vec<String>,
    #[arg(
        long,
        help = "Reusable image-generation recipe for missing theme variants"
    )]
    pub prompt: Option<String>,
    #[arg(long, conflicts_with = "all_themes")]
    pub theme: Option<String>,
    #[arg(long, help = "Mark this file as compatible with every theme")]
    pub all_themes: bool,
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct ArtifactEditArgs {
    pub name: String,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long)]
    pub alt_text: Option<String>,
    #[arg(long = "tag", help = "Replace all semantic tags")]
    pub tags: Vec<String>,
    #[arg(long, conflicts_with = "clear_prompt")]
    pub prompt: Option<String>,
    #[arg(long)]
    pub clear_prompt: bool,
    #[arg(long, requires = "to_theme")]
    pub from_theme: Option<String>,
    #[arg(long, requires = "from_theme")]
    pub to_theme: Option<String>,
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct ArtifactProjectArgs {
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct ArtifactNameArgs {
    pub name: String,
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum TemplateKindArg {
    Slides,
    Page,
}

#[derive(Debug, Args)]
pub struct TemplateCreateArgs {
    pub name: String,
    #[arg(long, value_enum)]
    pub kind: TemplateKindArg,
    #[arg(long = "from", value_name = "PATH")]
    pub source: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct TemplateListArgs {
    #[arg(long, value_enum)]
    pub kind: Option<TemplateKindArg>,
}

#[derive(Debug, Args)]
pub struct TemplateShowArgs {
    pub name: String,
    #[arg(long, value_enum)]
    pub kind: TemplateKindArg,
}

#[derive(Debug, Args)]
pub struct PluginNameArgs {
    pub id: String,
}

#[derive(Debug, Subcommand)]
pub enum EditCommands {
    Slides(EditSlidesArgs),
}

#[derive(Debug, Subcommand)]
pub enum ProjectCommands {
    List,
    Show(ProjectShowArgs),
    Use(ProjectNameArgs),
    Remove(ProjectNameArgs),
}

#[derive(Debug, Subcommand)]
pub enum ConnectorCommands {
    List,
    #[command(about = "List the connector presets available to `connector setup`")]
    Presets,
    Show(ConnectorShowArgs),
    #[command(about = "Show native features exposed by a connector")]
    Capabilities(ConnectorShowArgs),
    #[command(about = "Discover models available through a connector's native catalog")]
    Models(ConnectorShowArgs),
    #[command(about = "Show native account, usage, or local runtime status")]
    Status(ConnectorShowArgs),
    Setup(ConnectorSetupArgs),
    #[command(about = "Securely save a connector credential in the operating-system keyring")]
    Login(ConnectorShowArgs),
    #[command(about = "Check whether a connector credential is available")]
    AuthStatus(ConnectorShowArgs),
    #[command(about = "Remove a connector credential from secure storage")]
    Logout(ConnectorShowArgs),
}

#[derive(Debug, Subcommand)]
pub enum ModelCommands {
    Add(ModelAddArgs),
    Edit(ModelEditArgs),
    List,
    Show(ModelNameArgs),
    Remove(ModelNameArgs),
    Use(ModelUseArgs),
}

#[derive(Debug, Args)]
pub struct ModelEditArgs {
    pub name: String,

    #[arg(long)]
    pub connector: Option<String>,

    #[arg(long = "id", help = "Provider-specific model ID")]
    pub model_id: Option<String>,

    #[arg(long = "capability", help = "Replace the profile capabilities")]
    pub capabilities: Vec<String>,

    #[arg(
        long = "option",
        value_name = "KEY=VALUE",
        help = "Set or replace model options"
    )]
    pub options: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ModelAddArgs {
    pub name: String,

    #[arg(long)]
    pub connector: String,

    #[arg(long = "id", help = "Provider-specific model ID")]
    pub model_id: String,

    #[arg(long = "capability", required = true)]
    pub capabilities: Vec<String>,

    #[arg(long = "option", value_name = "KEY=VALUE")]
    pub options: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ModelNameArgs {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct ModelUseArgs {
    #[arg(help = "Capability or model role, for example text or reviewer")]
    pub selector: String,
    pub profile: String,

    #[arg(long, help = "Assign the default to this project instead of the user")]
    pub project: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ThemeCommands {
    Create(ThemeNameArgs),
    #[command(about = "Import a Google DESIGN.md file as a theme")]
    Import(ThemeImportArgs),
    #[command(about = "Export a theme as a Google DESIGN.md file")]
    Export(ThemeExportArgs),
    List,
    Show(ThemeNameArgs),
    Use(ThemeUseArgs),
}

#[derive(Debug, Args)]
pub struct ThemeImportArgs {
    pub path: PathBuf,
    #[arg(long, help = "Override the theme name derived from DESIGN.md")]
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct ThemeExportArgs {
    pub name: String,
    #[arg(long, default_value = "DESIGN.md")]
    pub out: PathBuf,
}

#[derive(Debug, Subcommand)]
pub enum PromptCommands {
    #[command(about = "List available prompt template IDs")]
    List(PromptProjectArgs),
    #[command(about = "Show the resolved source for a prompt template")]
    Show(PromptShowArgs),
    #[command(about = "Copy a bundled prompt into an editable override")]
    Customize(PromptCustomizeArgs),
    #[command(about = "Validate all resolved prompt templates")]
    Validate(PromptProjectArgs),
}

#[derive(Debug, Args)]
pub struct PromptProjectArgs {
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct PromptShowArgs {
    pub id: String,

    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct PromptCustomizeArgs {
    pub id: String,

    #[arg(long, value_enum)]
    pub scope: PromptScope,

    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PromptScope {
    User,
    Project,
}

#[derive(Debug, Args)]
pub struct ThemeNameArgs {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct ThemeUseArgs {
    pub name: String,

    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConnectorShowArgs {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct ConnectorSetupArgs {
    #[arg(value_enum)]
    pub preset: ConnectorPreset,

    #[arg(long, help = "Connector name; defaults to the preset name")]
    pub name: Option<String>,

    #[arg(
        long,
        help = "Use an environment variable instead of secure OS credential storage"
    )]
    pub api_key_env: Option<String>,
}

#[derive(Debug, Args)]
pub struct InitProjectArgs {
    pub name: String,

    #[arg(long, default_value = ".")]
    pub path: PathBuf,

    #[arg(long)]
    pub no_activate: bool,
}

#[derive(Debug, Args)]
pub struct ProjectShowArgs {
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct ProjectNameArgs {
    pub name: String,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommands {
    #[command(about = "Show the effective, user, or project config")]
    Show(ConfigShowArgs),
    #[command(about = "Set a config value by dotted key")]
    Set(ConfigSetArgs),
    #[command(about = "Delete a config value by dotted key")]
    Delete(ConfigDeleteArgs),
}

#[derive(Debug, Args)]
pub struct ConfigShowArgs {
    #[arg(help = "Optional dotted key, for example defaults.text")]
    pub key: Option<String>,

    #[arg(long, value_enum, default_value_t = ConfigScope::Effective, help = "Config scope to read from")]
    pub scope: ConfigScope,

    #[arg(long, help = "Project name when reading project or effective config")]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfigSetArgs {
    #[arg(help = "Dotted key to set, for example inference.model")]
    pub key: String,
    #[arg(help = "TOML value, or a plain string if TOML parsing fails")]
    pub value: String,

    #[arg(long, value_enum, default_value_t = ConfigScope::User, help = "Config file to edit")]
    pub scope: ConfigScope,

    #[arg(long, help = "Project name when editing project config")]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfigDeleteArgs {
    #[arg(help = "Dotted key to delete, for example defaults.text")]
    pub key: String,

    #[arg(long, value_enum, default_value_t = ConfigScope::User, help = "Config file to edit")]
    pub scope: ConfigScope,

    #[arg(long, help = "Project name when editing project config")]
    pub project: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum DocumentPageSizeArg {
    A4,
    Letter,
}

#[derive(Clone, Debug, Args)]
pub struct DocumentArgs {
    pub inputs: Vec<PathBuf>,

    #[arg(long, required = true)]
    pub instruction: String,

    #[arg(long)]
    pub title: Option<String>,

    #[arg(
        long,
        help = "Opt in to a reusable document structure for this generation"
    )]
    pub template: Option<String>,

    #[arg(
        long,
        help = "Publish the rendered PDF to this folder; working artifacts remain in Sfumato's project workspace"
    )]
    pub out: Option<PathBuf>,

    #[arg(
        long,
        value_enum,
        help = "Sheet to print on; the theme decides when omitted"
    )]
    pub page_size: Option<DocumentPageSizeArg>,

    #[arg(long, help = "Generate a table of contents", overrides_with = "no_toc")]
    pub toc: bool,

    #[arg(long = "no-toc", help = "Omit the table of contents")]
    pub no_toc: bool,

    #[arg(long, help = "Generate a cover page", overrides_with = "no_cover")]
    pub cover: bool,

    #[arg(long = "no-cover", help = "Omit the cover page")]
    pub no_cover: bool,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub project: Option<String>,

    #[arg(long)]
    pub theme: Option<String>,

    #[arg(long = "model", value_name = "CAPABILITY=PROFILE")]
    pub model_overrides: Vec<String>,

    #[arg(long, value_name = "PROFILE")]
    pub review_model: Option<String>,

    #[arg(long)]
    pub no_review: bool,

    #[arg(long)]
    pub json: bool,

    #[arg(
        long = "tool",
        value_enum,
        help = "Enable an optional generation tool for this request"
    )]
    pub tools: Vec<GenerationToolArg>,

    #[arg(
        long = "disable-tool",
        value_enum,
        help = "Disable a project generation tool for this request"
    )]
    pub disabled_tools: Vec<GenerationToolArg>,
}

#[derive(Clone, Debug, Args)]
pub struct SlidesArgs {
    pub inputs: Vec<PathBuf>,

    #[arg(long, required = true)]
    pub instruction: String,

    #[arg(long)]
    pub title: Option<String>,

    #[arg(
        long,
        help = "Opt in to a reusable slide structure for this generation"
    )]
    pub template: Option<String>,

    #[arg(
        long,
        help = "Publish the rendered PDF to this folder; working artifacts remain in Sfumato's project workspace"
    )]
    pub out: Option<PathBuf>,

    #[arg(long, hide = true)]
    pub pdf: bool,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub project: Option<String>,

    #[arg(long)]
    pub theme: Option<String>,

    #[arg(long = "model", value_name = "CAPABILITY=PROFILE")]
    pub model_overrides: Vec<String>,

    #[arg(long, value_name = "PROFILE")]
    pub review_model: Option<String>,

    #[arg(long)]
    pub no_review: bool,

    #[arg(long)]
    pub json: bool,

    #[arg(
        long = "tool",
        value_enum,
        help = "Enable an optional generation tool for this request"
    )]
    pub tools: Vec<GenerationToolArg>,

    #[arg(
        long = "disable-tool",
        value_enum,
        help = "Disable a project generation tool for this request"
    )]
    pub disabled_tools: Vec<GenerationToolArg>,
}

#[derive(Clone, Debug, Args)]
pub struct PageArgs {
    pub inputs: Vec<PathBuf>,

    #[arg(long, required = true)]
    pub instruction: String,

    #[arg(long)]
    pub title: Option<String>,

    #[arg(long, help = "Opt in to a reusable page structure for this generation")]
    pub template: Option<String>,

    #[arg(
        long,
        help = "Publish the page to this folder; managed revisions remain in Sfumato's project workspace"
    )]
    pub out: Option<PathBuf>,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub project: Option<String>,

    #[arg(long)]
    pub theme: Option<String>,

    #[arg(long = "model", value_name = "CAPABILITY=PROFILE")]
    pub model_overrides: Vec<String>,

    #[arg(long, value_name = "PROFILE")]
    pub review_model: Option<String>,

    #[arg(
        long = "plugin",
        value_name = "ID",
        help = "Enable an installed utility plugin"
    )]
    pub plugins: Vec<String>,

    #[arg(
        long = "disable-plugin",
        value_name = "ID",
        help = "Disable one project utility plugin for this request"
    )]
    pub disabled_plugins: Vec<String>,

    #[arg(
        long,
        value_name = "ID|none",
        help = "Override the exclusive project UI library"
    )]
    pub ui: Option<String>,

    #[arg(long, hide = true, help = "Deprecated: use --ui shadcn")]
    pub shadcn: bool,

    #[arg(long)]
    pub no_review: bool,

    #[arg(long)]
    pub json: bool,

    #[arg(
        long = "tool",
        value_enum,
        help = "Enable an optional generation tool for this request"
    )]
    pub tools: Vec<GenerationToolArg>,

    #[arg(
        long = "disable-tool",
        value_enum,
        help = "Disable a project generation tool for this request"
    )]
    pub disabled_tools: Vec<GenerationToolArg>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum VideoEngineArg {
    Hyperframe,
    Manim,
    Model,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum VideoAudioArg {
    Auto,
    On,
    Off,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum VideoWorkflowArg {
    #[default]
    Auto,
    Explainer,
    MotionGraphics,
    ProductLaunch,
    TalkingHead,
    Slideshow,
    General,
}

#[derive(Clone, Debug, Args)]
pub struct VideoArgs {
    pub inputs: Vec<PathBuf>,
    #[arg(
        long = "url",
        value_name = "URL",
        help = "Capture a website as a managed Hyperframe source"
    )]
    pub urls: Vec<String>,
    #[arg(long, required = true)]
    pub instruction: String,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long, value_enum)]
    pub engine: VideoEngineArg,
    #[arg(long, value_enum, default_value_t = VideoWorkflowArg::Auto)]
    pub workflow: VideoWorkflowArg,
    #[arg(long, required = true)]
    pub duration: u32,
    #[arg(long, help = "Publish the final MP4 under <folder>/_sfumato/videos")]
    pub out: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub project: Option<String>,
    #[arg(long)]
    pub theme: Option<String>,
    #[arg(long = "model", value_name = "CAPABILITY=PROFILE")]
    pub model_overrides: Vec<String>,
    #[arg(long, value_name = "PROFILE")]
    pub review_model: Option<String>,
    #[arg(long)]
    pub no_review: bool,
    #[arg(
        long,
        help = "Pause after contact-sheet review; use `sfumato video approve` to render"
    )]
    pub visual_review: bool,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub resolution: Option<String>,
    #[arg(long)]
    pub aspect_ratio: Option<String>,
    #[arg(long, help = "Local renderer frame rate (defaults to 30)")]
    pub fps: Option<u32>,
    #[arg(long, help = "Local renderer quality: draft, standard, or high")]
    pub quality: Option<String>,
    #[arg(
        long,
        value_enum,
        help = "Narration policy: on requires a speech profile, off renders silent"
    )]
    pub audio: Option<VideoAudioArg>,
    #[arg(
        long,
        value_name = "VOICE_ID",
        help = "Override the speech profile's voice for this film"
    )]
    pub voice: Option<String>,
    #[arg(long)]
    pub allow_code_execution: bool,
    #[arg(long = "tool", value_enum)]
    pub tools: Vec<GenerationToolArg>,
    #[arg(long = "disable-tool", value_enum)]
    pub disabled_tools: Vec<GenerationToolArg>,
}

#[derive(Clone, Debug, Args)]
pub struct EditSlidesArgs {
    #[arg(value_name = "DECK", help = "Generated Marp Markdown deck to update")]
    pub markdown_path: PathBuf,

    #[arg(long, required = true)]
    pub instruction: String,

    #[arg(long)]
    pub project: Option<String>,

    #[arg(long = "model", value_name = "text=PROFILE")]
    pub model_overrides: Vec<String>,

    #[arg(long)]
    pub json: bool,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum ConfigScope {
    User,
    Project,
    Effective,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ConnectorPreset {
    Ollama,
    Lmstudio,
    Openrouter,
    Anthropic,
    Codex,
    Elevenlabs,
}

impl From<ConnectorPreset> for sfumato_core::connectors::ConnectorPreset {
    fn from(preset: ConnectorPreset) -> Self {
        // Clap forbids `sfumato-core` from depending on it, so the preset list is
        // mirrored here. `tests/unit/cli.rs` asserts the two stay in sync.
        match preset {
            ConnectorPreset::Ollama => Self::Ollama,
            ConnectorPreset::Lmstudio => Self::Lmstudio,
            ConnectorPreset::Openrouter => Self::Openrouter,
            ConnectorPreset::Anthropic => Self::Anthropic,
            ConnectorPreset::Codex => Self::Codex,
            ConnectorPreset::Elevenlabs => Self::Elevenlabs,
        }
    }
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests inspect private CLI types.
#[path = "../tests/unit/cli.rs"]
mod tests;
