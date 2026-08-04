use std::{str::FromStr, sync::Arc};

use crate::{
    config::{Capability, GlobalConfig, ModelOptions, ModelProfile, ModelRole},
    errors::{NotFoundContext, ResultContext as Context, SfumatoError, SfumatoResult as Result},
    repositories::{GlobalConfigRepository, ProjectRepository},
    sfumato_bail as bail,
};

pub struct ModelService {
    config: GlobalConfig,
    revision: String,
    global_repository: Arc<dyn GlobalConfigRepository>,
    project_repository: Arc<dyn ProjectRepository>,
}

#[derive(Clone, Debug)]
pub struct ModelSummary {
    pub name: String,
    pub connector: String,
    pub model: String,
    pub capabilities: Vec<Capability>,
}

#[derive(Clone, Debug)]
pub struct ModelDefaultChanged {
    pub selection: ModelSelection,
    pub profile: String,
    pub project: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelSelection {
    Capability(Capability),
    Role(ModelRole),
}

impl ModelSelection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capability(capability) => capability.as_str(),
            Self::Role(role) => role.as_str(),
        }
    }

    fn required_capability(self) -> Capability {
        match self {
            Self::Capability(capability) => capability,
            Self::Role(role) => role.required_capability(),
        }
    }
}

impl FromStr for ModelSelection {
    type Err = SfumatoError;

    fn from_str(value: &str) -> Result<Self> {
        Capability::from_str(value)
            .map(Self::Capability)
            .or_else(|_| ModelRole::from_str(value).map(Self::Role))
            .map_err(|_| {
                SfumatoError::validation(
                    "Unknown model capability or role '{value}'. Use text, code, image, video, speech, embedding, or reviewer."
                )
            })
    }
}

impl ModelService {
    pub fn new(
        global_repository: Arc<dyn GlobalConfigRepository>,
        project_repository: Arc<dyn ProjectRepository>,
    ) -> Result<Self> {
        let snapshot = global_repository.load_snapshot()?;
        Ok(Self {
            config: snapshot.value,
            revision: snapshot.revision,
            global_repository,
            project_repository,
        })
    }

    pub fn list(&self) -> Vec<ModelSummary> {
        self.config
            .models
            .iter()
            .map(|(name, profile)| ModelSummary {
                name: name.clone(),
                connector: profile.connector.clone(),
                model: profile.model.clone(),
                capabilities: profile.capabilities.clone(),
            })
            .collect()
    }

    pub fn profile(&self, name: &str) -> Result<ModelProfile> {
        self.config
            .models
            .get(name)
            .cloned()
            .or_not_found_with(|| format!("Model profile '{name}' was not found"))
    }

    pub fn add(
        &mut self,
        name: String,
        connector: String,
        model_id: String,
        capabilities: Vec<String>,
        options: Vec<String>,
    ) -> Result<ModelProfile> {
        validate_profile_name(&name)?;
        if self.config.models.contains_key(&name) {
            bail!("Model profile '{name}' already exists");
        }
        if !self.config.connectors.contains_key(&connector) {
            return Err(SfumatoError::not_found(format!(
                "Connector '{connector}' was not found"
            )));
        }
        if model_id.trim().is_empty() {
            bail!("Model ID cannot be empty");
        }
        let capabilities = parse_capabilities(&capabilities)?;
        let options = parse_options(&options)?;
        let profile = ModelProfile {
            connector,
            model: model_id,
            capabilities,
            options,
        };
        self.config.models.insert(name.clone(), profile.clone());
        self.save()?;
        Ok(profile)
    }

    pub fn remove(&mut self, name: &str) -> Result<String> {
        if !self.config.models.contains_key(name) {
            return Err(SfumatoError::not_found(format!(
                "Model profile '{name}' was not found"
            )));
        }
        if let Some(capability) = self
            .config
            .defaults
            .0
            .iter()
            .find_map(|(capability, profile)| (profile == name).then_some(*capability))
        {
            bail!(
                "Model profile '{name}' is the user default for '{}'; select another default first",
                capability.as_str()
            );
        }
        if let Some(role) = self
            .config
            .model_roles
            .iter()
            .find_map(|(role, profile)| (profile == name).then_some(*role))
        {
            bail!(
                "Model profile '{name}' is the user default for '{}'; select another default first",
                role.as_str()
            );
        }
        for (project_name, _, _) in self.project_repository.list()? {
            // A project whose config cannot be read holds no default that could
            // block this removal, and refusing to remove a profile because of an
            // unrelated broken project leaves no way forward.
            let Ok(project) = self.project_repository.load(Some(&project_name)) else {
                continue;
            };
            if let Some(capability) = project
                .model_defaults
                .iter()
                .find_map(|(capability, profile)| (profile == name).then_some(*capability))
            {
                bail!(
                    "Model profile '{name}' is the '{}' default for project '{project_name}'; select another default first",
                    capability.as_str()
                );
            }
            if let Some(role) = project
                .model_roles
                .iter()
                .find_map(|(role, profile)| (profile == name).then_some(*role))
            {
                bail!(
                    "Model profile '{name}' is the '{}' default for project '{project_name}'; select another default first",
                    role.as_str()
                );
            }
        }
        self.config.models.remove(name);
        self.save()?;
        Ok(name.to_string())
    }

    pub fn edit(
        &mut self,
        name: &str,
        connector: Option<String>,
        model_id: Option<String>,
        capabilities: Vec<String>,
        options: Vec<String>,
    ) -> Result<ModelProfile> {
        if connector.is_none()
            && model_id.is_none()
            && capabilities.is_empty()
            && options.is_empty()
        {
            bail!("No model profile changes were provided");
        }
        if let Some(connector) = &connector
            && !self.config.connectors.contains_key(connector)
        {
            return Err(SfumatoError::not_found(format!(
                "Connector '{connector}' was not found"
            )));
        }
        if model_id
            .as_ref()
            .is_some_and(|model_id| model_id.trim().is_empty())
        {
            bail!("Model ID cannot be empty");
        }
        let parsed_capabilities = (!capabilities.is_empty())
            .then(|| parse_capabilities(&capabilities))
            .transpose()?;
        let parsed_options = parse_options(&options)?;
        let mut profile = self
            .config
            .models
            .get(name)
            .cloned()
            .or_not_found_with(|| format!("Model profile '{name}' was not found"))?;
        if let Some(connector) = connector {
            profile.connector = connector;
        }
        if let Some(model_id) = model_id {
            profile.model = model_id;
        }
        if let Some(capabilities) = parsed_capabilities {
            profile.capabilities = capabilities;
        }
        profile.options.merge(parsed_options);
        let mut updated_config = self.config.clone();
        updated_config.models.insert(name.to_string(), profile);
        validate_selected_capabilities(&updated_config, name, self.project_repository.as_ref())?;
        self.config = updated_config;
        self.save()?;
        self.profile(name)
    }

    pub fn use_default(
        &mut self,
        selector: &str,
        profile_name: &str,
        project: Option<&str>,
    ) -> Result<ModelDefaultChanged> {
        let selection = ModelSelection::from_str(selector)?;
        let required = selection.required_capability();
        let profile = self
            .config
            .models
            .get(profile_name)
            .or_not_found_with(|| format!("Model profile '{profile_name}' was not found"))?;
        if !profile.capabilities.contains(&required) {
            bail!(
                "Model profile '{profile_name}' does not support '{}' capability",
                required.as_str()
            );
        }

        if let Some(project_name) = project {
            let snapshot = self.project_repository.load_snapshot(Some(project_name))?;
            let mut project_config = snapshot.value;
            match selection {
                ModelSelection::Capability(capability) => {
                    project_config
                        .model_defaults
                        .insert(capability, profile_name.to_string());
                }
                ModelSelection::Role(role) => {
                    project_config
                        .model_roles
                        .insert(role, profile_name.to_string());
                }
            }
            self.project_repository
                .save_if_revision(&project_config, &snapshot.revision)?;
            return Ok(ModelDefaultChanged {
                selection,
                profile: profile_name.to_string(),
                project: Some(project_config.name),
            });
        } else {
            match selection {
                ModelSelection::Capability(capability) => {
                    self.config
                        .defaults
                        .0
                        .insert(capability, profile_name.to_string());
                }
                ModelSelection::Role(role) => {
                    self.config
                        .model_roles
                        .insert(role, profile_name.to_string());
                }
            }
            self.save()?;
        }
        Ok(ModelDefaultChanged {
            selection,
            profile: profile_name.to_string(),
            project: None,
        })
    }

    fn save(&mut self) -> Result<()> {
        self.revision = self
            .global_repository
            .save_if_revision(&self.config, &self.revision)?;
        Ok(())
    }
}

fn validate_selected_capabilities(
    config: &GlobalConfig,
    profile_name: &str,
    project_repository: &dyn ProjectRepository,
) -> Result<()> {
    let profile = config
        .models
        .get(profile_name)
        .context("Edited model profile was not found")?;
    for (capability, selected_profile) in &config.defaults.0 {
        if selected_profile == profile_name && !profile.capabilities.contains(capability) {
            bail!(
                "Cannot remove '{}' capability because '{profile_name}' is the user default for it",
                capability.as_str()
            );
        }
    }
    for (role, selected_profile) in &config.model_roles {
        if selected_profile == profile_name
            && !profile.capabilities.contains(&role.required_capability())
        {
            bail!(
                "Cannot remove '{}' capability because '{profile_name}' is the user default for '{}'",
                role.required_capability().as_str(),
                role.as_str()
            );
        }
    }
    for (project_name, _, _) in project_repository.list()? {
        // A project whose config cannot be read references no profile, so it
        // cannot be the reason to reject this edit. Aborting here made every
        // command that validates capabilities fail because of an unrelated
        // project, including `project remove`, the way out of that state.
        let Ok(project) = project_repository.load(Some(&project_name)) else {
            continue;
        };
        for (capability, selected_profile) in project.model_defaults {
            if selected_profile == profile_name && !profile.capabilities.contains(&capability) {
                bail!(
                    "Cannot remove '{}' capability because '{profile_name}' is the default for project '{project_name}'",
                    capability.as_str()
                );
            }
        }
        for (role, selected_profile) in project.model_roles {
            if selected_profile == profile_name
                && !profile.capabilities.contains(&role.required_capability())
            {
                bail!(
                    "Cannot remove '{}' capability because '{profile_name}' is the '{}' default for project '{project_name}'",
                    role.required_capability().as_str(),
                    role.as_str()
                );
            }
        }
    }
    Ok(())
}

fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        || name.starts_with('-')
        || name.ends_with('-')
    {
        bail!("Invalid model profile name '{name}'. Use lowercase letters, numbers, and hyphens.");
    }
    Ok(())
}

fn parse_capabilities(values: &[String]) -> Result<Vec<Capability>> {
    let mut parsed = values
        .iter()
        .map(|value| Capability::from_str(value.trim()).map_err(SfumatoError::validation))
        .collect::<Result<Vec<_>>>()?;
    parsed.sort();
    parsed.dedup();
    if parsed.is_empty() {
        bail!("Model profile must support at least one capability");
    }
    Ok(parsed)
}

fn parse_options(values: &[String]) -> Result<ModelOptions> {
    let mut options = ModelOptions::default();
    for value in values {
        let (key, raw) = value
            .split_once('=')
            .with_context(|| format!("Invalid model option '{value}'. Use key=value."))?;
        let key = key.trim();
        let raw = raw.trim();
        match key {
            "temperature" => options.text.temperature = Some(parse_option(raw, key)?),
            "max_tokens" => options.text.max_tokens = Some(parse_option(raw, key)?),
            "max_tool_rounds" => options.text.max_tool_rounds = Some(parse_option(raw, key)?),
            "top_p" => options.text.top_p = Some(parse_option(raw, key)?),
            "seed" => options.text.seed = Some(parse_option(raw, key)?),
            "quality" => options.image.quality = Some(required_option_string(raw, key)?),
            "background" => options.image.background = Some(required_option_string(raw, key)?),
            "size" => options.image.size = Some(required_option_string(raw, key)?),
            "aspect_ratio" => options.image.aspect_ratio = Some(required_option_string(raw, key)?),
            "output_format" => {
                options.image.output_format = Some(required_option_string(raw, key)?)
            }
            "video_duration_seconds" => {
                options.video.duration_seconds = Some(parse_option(raw, key)?)
            }
            "video_resolution" => {
                options.video.resolution = Some(required_option_string(raw, key)?)
            }
            "video_aspect_ratio" => {
                options.video.aspect_ratio = Some(required_option_string(raw, key)?)
            }
            "video_audio" => options.video.audio = Some(raw.parse()?),
            "video_seed" => options.video.seed = Some(parse_option(raw, key)?),
            "video_poll_interval_seconds" => {
                options.video.poll_interval_seconds = Some(parse_option(raw, key)?)
            }
            "video_timeout_seconds" => {
                options.video.timeout_seconds = Some(parse_option(raw, key)?)
            }
            "speech_voice" => options.speech.voice = Some(required_option_string(raw, key)?),
            "speech_output_format" => {
                options.speech.output_format = Some(required_option_string(raw, key)?)
            }
            "speech_language" => options.speech.language = Some(required_option_string(raw, key)?),
            "speech_stability" => options.speech.stability = Some(parse_option(raw, key)?),
            "speech_similarity_boost" => {
                options.speech.similarity_boost = Some(parse_option(raw, key)?)
            }
            "speech_style" => options.speech.style = Some(parse_option(raw, key)?),
            "speech_speed" => options.speech.speed = Some(parse_option(raw, key)?),
            "speech_speaker_boost" => options.speech.speaker_boost = Some(parse_option(raw, key)?),
            "speech_segment_gap_seconds" => {
                options.speech.segment_gap_seconds = Some(parse_option(raw, key)?)
            }
            "" => bail!("Model option key cannot be empty"),
            _ => bail!(
                "Unknown model option '{key}'. Supported options include text/image options, video_duration_seconds, video_resolution, video_aspect_ratio, video_audio, video_seed, video_poll_interval_seconds, video_timeout_seconds, and speech_voice, speech_output_format, speech_language, speech_stability, speech_similarity_boost, speech_style, speech_speed, speech_speaker_boost, speech_segment_gap_seconds."
            ),
        }
    }
    Ok(options)
}

fn parse_option<T>(raw: &str, key: &str) -> Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    raw.parse::<T>().map_err(|error| {
        SfumatoError::validation(format!(
            "Model option '{key}' has invalid value '{raw}': {error}"
        ))
    })
}

fn required_option_string(raw: &str, key: &str) -> Result<String> {
    if raw.is_empty() {
        bail!("Model option '{key}' cannot be empty");
    }
    Ok(raw.to_string())
}

#[cfg(test)]
// Test bodies live under tests/unit so implementation files stay focused, while
// this module hook still lets those tests exercise private helpers.
#[path = "../tests/unit/models.rs"]
mod tests;
