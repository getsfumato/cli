use std::path::PathBuf;

use anyhow::Result;
use inquire::{Confirm, InquireError, Select, Text};

use crate::cli::{
    Commands, ConfigCommands, ConfigDeleteArgs, ConfigScope, ConfigSetArgs, ConfigShowArgs,
    ConnectorCommands, ConnectorPreset, ConnectorSetupArgs, ConnectorShowArgs, GenerateCommands,
    InitProjectArgs, InitTarget, ModelAddArgs, ModelCommands, ModelEditArgs, ModelNameArgs,
    ModelUseArgs, ProjectCommands, ProjectNameArgs, ProjectShowArgs, SlidesArgs, ThemeCommands,
    ThemeNameArgs, ThemeUseArgs,
};
use sfumato_core::models::ModelService;

const MAIN_MENU: &[&str] = &[
    "Generate resources",
    "Projects",
    "Themes",
    "Connectors",
    "Models",
    "Configuration",
    "Setup",
    "Exit",
];

const PROJECT_MENU: &[&str] = &["List", "Show", "Create", "Use", "Remove", "Back"];

pub fn welcome() {
    println!("Sfumato\nGenerate themed learning resources from your terminal.\n");
}

pub fn next_command() -> Result<Option<Commands>> {
    loop {
        let Some(section) = select("What would you like to do?", MAIN_MENU)? else {
            return Ok(None);
        };
        let command = match section.as_str() {
            "Generate resources" => generate_menu()?,
            "Projects" => project_menu()?,
            "Themes" => theme_menu()?,
            "Connectors" => connector_menu()?,
            "Models" => model_menu()?,
            "Configuration" => config_menu()?,
            "Setup" => setup_menu()?,
            "Exit" => return Ok(None),
            _ => unreachable!("main menu choices are fixed"),
        };
        if command.is_some() {
            return Ok(command);
        }
    }
}

fn model_menu() -> Result<Option<Commands>> {
    let Some(action) = select(
        "Models",
        &["List", "Show", "Add", "Edit", "Use", "Remove", "Back"],
    )?
    else {
        return Ok(None);
    };
    let command = match action.as_str() {
        "List" => ModelCommands::List,
        "Show" => {
            let Some(name) = required_text("Model profile name", None)? else {
                return Ok(None);
            };
            ModelCommands::Show(ModelNameArgs { name })
        }
        "Add" => {
            let Some(name) = required_text("Model profile name", None)? else {
                return Ok(None);
            };
            let Some(connector) = required_text("Connector name", None)? else {
                return Ok(None);
            };
            let Some(model_id) = required_text("Provider model ID", None)? else {
                return Ok(None);
            };
            let Some(capabilities) =
                required_text("Capabilities, separated by commas", Some("text,code"))?
            else {
                return Ok(None);
            };
            ModelCommands::Add(ModelAddArgs {
                name,
                connector,
                model_id,
                capabilities: parse_list(&capabilities),
                options: optional_text("Options as key=value, separated by commas", None)?
                    .map(|value| parse_list(&value))
                    .unwrap_or_default(),
            })
        }
        "Edit" => {
            let Some(name) = required_text("Model profile name", None)? else {
                return Ok(None);
            };
            let profile = ModelService::load()?.profile(&name)?;
            let capabilities = profile
                .capabilities
                .iter()
                .map(|capability| capability.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let options = profile
                .options
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(",");
            let connector = optional_text("Connector name", Some(&profile.connector))?;
            let model_id = optional_text("Provider model ID", Some(&profile.model))?;
            let capabilities =
                optional_text("Capabilities, separated by commas", Some(&capabilities))?
                    .map(|value| parse_list(&value))
                    .unwrap_or_default();
            let options = optional_text(
                "Options to set as key=value, separated by commas",
                Some(&options),
            )?
            .map(|value| parse_list(&value))
            .unwrap_or_default();
            if connector.is_none()
                && model_id.is_none()
                && capabilities.is_empty()
                && options.is_empty()
            {
                return Ok(None);
            }
            ModelCommands::Edit(ModelEditArgs {
                name,
                connector,
                model_id,
                capabilities,
                options,
            })
        }
        "Use" => {
            let Some(capability) = required_text("Capability", Some("text"))? else {
                return Ok(None);
            };
            let Some(profile) = required_text("Model profile name", None)? else {
                return Ok(None);
            };
            let Some(scope) = select("Default scope", &["User", "Project"])? else {
                return Ok(None);
            };
            let project = if scope == "Project" {
                let Some(project) = required_text("Project name", None)? else {
                    return Ok(None);
                };
                Some(project)
            } else {
                None
            };
            ModelCommands::Use(ModelUseArgs {
                capability,
                profile,
                project,
            })
        }
        "Remove" => {
            let Some(name) = required_text("Model profile name", None)? else {
                return Ok(None);
            };
            ModelCommands::Remove(ModelNameArgs { name })
        }
        "Back" => return Ok(None),
        _ => unreachable!("model menu choices are fixed"),
    };
    Ok(Some(Commands::Model { command }))
}

fn generate_menu() -> Result<Option<Commands>> {
    let Some(action) = select("Generate resources", &["Slides", "Back"])? else {
        return Ok(None);
    };
    if action == "Back" {
        return Ok(None);
    }
    let Some(instruction) = required_text("Instruction", None)? else {
        return Ok(None);
    };
    let inputs = optional_text("Source files or folders, separated by commas", None)?
        .map(|value| parse_paths(&value))
        .unwrap_or_default();
    let title = optional_text("Title", None)?;
    let project = optional_text("Project override", None)?;
    let theme = optional_text("Theme override", None)?;
    let out = optional_text("Output folder override", None)?.map(PathBuf::from);
    let model_overrides = optional_text("Model overrides, separated by commas", None)?
        .map(|value| parse_list(&value))
        .unwrap_or_default();
    Ok(Some(Commands::Generate {
        command: GenerateCommands::Slides(SlidesArgs {
            inputs,
            instruction,
            title,
            out,
            pdf: confirm("Render PDF with Marp?", false)?,
            dry_run: confirm("Preview prompt without generating files?", false)?,
            project,
            theme,
            model_overrides,
            json: confirm("Print machine-readable JSON output?", false)?,
        }),
    }))
}

fn project_menu() -> Result<Option<Commands>> {
    let Some(action) = select("Projects", PROJECT_MENU)? else {
        return Ok(None);
    };
    let command = match action.as_str() {
        "List" => ProjectCommands::List,
        "Show" => ProjectCommands::Show(ProjectShowArgs {
            name: optional_text("Project name; leave empty for active project", None)?,
        }),
        "Create" => return init_project_command(),
        "Use" => {
            let Some(name) = required_text("Project name", None)? else {
                return Ok(None);
            };
            ProjectCommands::Use(ProjectNameArgs { name })
        }
        "Remove" => {
            let Some(name) = required_text("Project name", None)? else {
                return Ok(None);
            };
            ProjectCommands::Remove(ProjectNameArgs { name })
        }
        "Back" => return Ok(None),
        _ => unreachable!("project menu choices are fixed"),
    };
    Ok(Some(Commands::Project { command }))
}

fn theme_menu() -> Result<Option<Commands>> {
    let Some(action) = select("Themes", &["List", "Show", "Create", "Use", "Back"])? else {
        return Ok(None);
    };
    let command = match action.as_str() {
        "List" => ThemeCommands::List,
        "Show" => {
            let Some(name) = required_text("Theme name", None)? else {
                return Ok(None);
            };
            ThemeCommands::Show(ThemeNameArgs { name })
        }
        "Create" => {
            let Some(name) = required_text("New theme name", None)? else {
                return Ok(None);
            };
            ThemeCommands::Create(ThemeNameArgs { name })
        }
        "Use" => {
            let Some(name) = required_text("Theme name", None)? else {
                return Ok(None);
            };
            ThemeCommands::Use(ThemeUseArgs {
                name,
                project: optional_text("Project name; leave empty for active project", None)?,
            })
        }
        "Back" => return Ok(None),
        _ => unreachable!("theme menu choices are fixed"),
    };
    Ok(Some(Commands::Theme { command }))
}

fn connector_menu() -> Result<Option<Commands>> {
    let Some(action) = select("Connectors", &["List", "Show", "Setup", "Back"])? else {
        return Ok(None);
    };
    let command = match action.as_str() {
        "List" => ConnectorCommands::List,
        "Show" => {
            let Some(name) = required_text("Connector name", None)? else {
                return Ok(None);
            };
            ConnectorCommands::Show(ConnectorShowArgs { name })
        }
        "Setup" => {
            let Some(preset) = select("Connector preset", &["Ollama", "OpenRouter"])? else {
                return Ok(None);
            };
            ConnectorCommands::Setup(ConnectorSetupArgs {
                preset: if preset == "Ollama" {
                    ConnectorPreset::Ollama
                } else {
                    ConnectorPreset::Openrouter
                },
                name: optional_text("Connector name; leave empty for preset name", None)?,
                api_key_env: optional_text(
                    "OpenRouter API key environment variable",
                    Some("OPENROUTER_API_KEY"),
                )?
                .unwrap_or_else(|| "OPENROUTER_API_KEY".to_string()),
            })
        }
        "Back" => return Ok(None),
        _ => unreachable!("connector menu choices are fixed"),
    };
    Ok(Some(Commands::Connector { command }))
}

fn config_menu() -> Result<Option<Commands>> {
    let Some(action) = select("Configuration", &["Show", "Set", "Delete", "Back"])? else {
        return Ok(None);
    };
    if action == "Back" {
        return Ok(None);
    }
    let Some(scope) = select_scope(action == "Show")? else {
        return Ok(None);
    };
    let project = if matches!(scope, ConfigScope::Project | ConfigScope::Effective) {
        optional_text("Project name; leave empty for active project", None)?
    } else {
        None
    };
    let command = match action.as_str() {
        "Show" => ConfigCommands::Show(ConfigShowArgs {
            key: optional_text("Dotted key; leave empty to show everything", None)?,
            scope,
            project,
        }),
        "Set" => {
            let Some(key) = required_text("Dotted key", None)? else {
                return Ok(None);
            };
            let Some(value) = required_text("TOML value or string", None)? else {
                return Ok(None);
            };
            ConfigCommands::Set(ConfigSetArgs {
                key,
                value,
                scope,
                project,
            })
        }
        "Delete" => {
            let Some(key) = required_text("Dotted key", None)? else {
                return Ok(None);
            };
            ConfigCommands::Delete(ConfigDeleteArgs {
                key,
                scope,
                project,
            })
        }
        _ => unreachable!("config menu choices are fixed"),
    };
    Ok(Some(Commands::Config { command }))
}

fn setup_menu() -> Result<Option<Commands>> {
    let Some(action) = select("Setup", &["Initialize user", "Initialize project", "Back"])? else {
        return Ok(None);
    };
    match action.as_str() {
        "Initialize user" => Ok(Some(Commands::Init {
            target: InitTarget::User {
                yes: confirm("Use default values without setup questions?", false)?,
                force: confirm("Overwrite an existing user configuration?", false)?,
            },
        })),
        "Initialize project" => init_project_command(),
        "Back" => Ok(None),
        _ => unreachable!("setup menu choices are fixed"),
    }
}

fn init_project_command() -> Result<Option<Commands>> {
    let Some(name) = required_text("Project name", None)? else {
        return Ok(None);
    };
    let path = optional_text("Project path", Some("."))?
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(Some(Commands::Init {
        target: InitTarget::Project(InitProjectArgs {
            name,
            path,
            no_activate: !confirm("Make this the active project?", true)?,
        }),
    }))
}

fn select_scope(include_effective: bool) -> Result<Option<ConfigScope>> {
    let options = if include_effective {
        vec!["Effective", "User", "Project"]
    } else {
        vec!["User", "Project"]
    };
    Ok(match select("Configuration scope", &options)?.as_deref() {
        Some("User") => Some(ConfigScope::User),
        Some("Project") => Some(ConfigScope::Project),
        Some("Effective") => Some(ConfigScope::Effective),
        _ => None,
    })
}

fn select(message: &str, options: &[&str]) -> Result<Option<String>> {
    match Select::new(
        message,
        options.iter().map(|option| (*option).to_string()).collect(),
    )
    .with_help_message("Use arrow keys and Enter. Press Esc to go back.")
    .prompt()
    {
        Ok(value) => Ok(Some(value)),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn required_text(message: &str, default: Option<&str>) -> Result<Option<String>> {
    let mut prompt = Text::new(message);
    if let Some(default) = default {
        prompt = prompt.with_default(default);
    }
    match prompt.prompt() {
        Ok(value) if value.trim().is_empty() => Ok(None),
        Ok(value) => Ok(Some(value.trim().to_string())),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn optional_text(message: &str, default: Option<&str>) -> Result<Option<String>> {
    required_text(message, default)
}

fn confirm(message: &str, default: bool) -> Result<bool> {
    match Confirm::new(message).with_default(default).prompt() {
        Ok(value) => Ok(value),
        Err(InquireError::OperationCanceled | InquireError::OperationInterrupted) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

fn parse_paths(value: &str) -> Vec<PathBuf> {
    parse_list(value).into_iter().map(PathBuf::from).collect()
}

fn parse_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/menu.rs"]
mod tests;
