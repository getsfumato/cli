//! Production provider routing across HTTP and local process transports.

use std::sync::Arc;

use async_trait::async_trait;
use sfumato_core::{
    config::{
        AnthropicConnectorConfig, Capability, ConnectorConfig, EffectiveConfig,
        LmStudioConnectorConfig, ModelProfile, OllamaConnectorConfig, OpenRouterConnectorConfig,
    },
    errors::{SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{
        AgentRunner, ConnectorCapabilities, ConnectorCapability, ConnectorIntrospection,
        ConnectorModelSummary, ConnectorStatus, ImageGenerationProvider, ProviderFactory,
        TextGenerationProvider, VideoGenerationProvider,
    },
    secrets::SecretResolver,
};

use crate::{
    anthropic::AnthropicConnector,
    codex_app_server::{CodexAppServerImageProvider, CodexAppServerProvider},
    lmstudio::LmStudioConnector, ollama::OllamaConnector,
    openai_compatible::OpenAiCompatibleProviderFactory, openrouter::OpenRouterConnector,
};

/// Dispatches resolved model profiles to their configured transport adapter.
#[derive(Clone)]
pub struct AdapterProviderFactory {
    openai_compatible: OpenAiCompatibleProviderFactory,
    secrets: Arc<dyn SecretResolver>,
}

impl AdapterProviderFactory {
    /// Creates the production provider router.
    pub fn new(secrets: Arc<dyn SecretResolver>) -> Self {
        Self {
            openai_compatible: OpenAiCompatibleProviderFactory::new(secrets.clone()),
            secrets,
        }
    }

    fn connector<'a>(
        config: &'a EffectiveConfig,
        profile: &ModelProfile,
    ) -> SfumatoResult<&'a ConnectorConfig> {
        config.connectors.get(&profile.connector).ok_or_else(|| {
            SfumatoError::config(format_args!(
                "Connector '{}' was not found",
                profile.connector
            ))
        })
    }
}

impl ProviderFactory for AdapterProviderFactory {
    fn text(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> SfumatoResult<Box<dyn TextGenerationProvider>> {
        if !profile.capabilities.contains(&Capability::Text) {
            return Err(SfumatoError::config(
                "Selected model profile does not support text generation",
            ));
        }
        match Self::connector(config, profile)? {
            ConnectorConfig::OpenAiCompatible(_)
            | ConnectorConfig::OpenRouter(_)
            | ConnectorConfig::Ollama(_)
            | ConnectorConfig::LmStudio(_) => self.openai_compatible.text(config, profile),
            ConnectorConfig::Anthropic(connector) => {
                let model = AnthropicConnector::new(connector.clone(), self.secrets.clone())?
                    .text_model(profile.clone());
                Ok(Box::new(AgentRunner::new(Arc::new(model))) as Box<dyn TextGenerationProvider>)
            }
            ConnectorConfig::CodexAppServer(connector) => {
                Ok(Box::new(CodexAppServerProvider::new(
                    connector.clone(),
                    profile.clone(),
                    config.project_root.clone(),
                )))
            }
        }
    }

    fn image(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> SfumatoResult<Box<dyn ImageGenerationProvider>> {
        match Self::connector(config, profile)? {
            ConnectorConfig::OpenAiCompatible(_)
            | ConnectorConfig::OpenRouter(_)
            | ConnectorConfig::Ollama(_)
            | ConnectorConfig::LmStudio(_) => self.openai_compatible.image(config, profile),
            ConnectorConfig::Anthropic(_) => Err(SfumatoError::config(
                "Anthropic exposes no image-generation endpoint; configure an OpenRouter or OpenAI-compatible image profile",
            )),
            // Indirect: an agent turn that has to choose to invoke its own image
            // tool, rather than an endpoint that returns bytes. It runs on Codex's
            // authentication instead of a metered image endpoint, and a turn that
            // answers in prose is reported as a tool failure.
            ConnectorConfig::CodexAppServer(connector) => {
                Ok(Box::new(CodexAppServerImageProvider::new(
                    connector.clone(),
                    profile.clone(),
                    config.project_root.clone(),
                )))
            }
        }
    }

    fn video(
        &self,
        config: &EffectiveConfig,
        profile: &ModelProfile,
    ) -> SfumatoResult<Box<dyn VideoGenerationProvider>> {
        if !profile.capabilities.contains(&Capability::Video) {
            return Err(SfumatoError::config(
                "Selected model profile does not support video generation",
            ));
        }
        match Self::connector(config, profile)? {
            ConnectorConfig::OpenRouter(connector) => Ok(Box::new(
                OpenRouterConnector::new(&profile.connector, connector, self.secrets.clone())?
                    .video_provider(profile.clone()),
            )),
            _ => Err(SfumatoError::config(
                "Video generation currently requires an OpenRouter connector",
            )),
        }
    }
}

#[async_trait]
impl ConnectorIntrospection for AdapterProviderFactory {
    fn capabilities(&self, connector: &ConnectorConfig) -> ConnectorCapabilities {
        let (kind, features) = match native_connector(connector) {
            NativeConnector::OpenRouter(_) => (
                "openrouter",
                vec![
                    ConnectorCapability::ModelCatalog,
                    ConnectorCapability::Account,
                    ConnectorCapability::Usage,
                    ConnectorCapability::VideoGeneration,
                ],
            ),
            NativeConnector::Ollama(_) => (
                "ollama",
                vec![
                    ConnectorCapability::ModelCatalog,
                    ConnectorCapability::RuntimeStatus,
                ],
            ),
            // `ModelDetails` is deliberately absent: `/api/v0/models` does return
            // richer per-model data, but it lands in `ConnectorModelSummary`
            // metadata and no port method exposes a per-model detail operation.
            // ADR-0008 requires capabilities to name implemented operations only.
            NativeConnector::LmStudio(_) => (
                "lmstudio",
                vec![
                    ConnectorCapability::ModelCatalog,
                    ConnectorCapability::RuntimeStatus,
                ],
            ),
            // `Account` and `Usage` are deliberately absent: spend and identity
            // live behind the Admin API, which a Messages key cannot reach.
            NativeConnector::Anthropic(_) => ("anthropic", vec![ConnectorCapability::ModelCatalog]),
            NativeConnector::Codex => (
                "codex_app_server",
                vec![
                    ConnectorCapability::ModelCatalog,
                    ConnectorCapability::Account,
                    ConnectorCapability::Usage,
                ],
            ),
            NativeConnector::Generic => ("openai_compatible", Vec::new()),
        };
        ConnectorCapabilities {
            kind: kind.into(),
            features,
        }
    }

    async fn list_models(
        &self,
        connector_name: &str,
        connector: &ConnectorConfig,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<ConnectorModelSummary>> {
        match native_connector(connector) {
            NativeConnector::OpenRouter(config) => {
                OpenRouterConnector::new(connector_name, &config, self.secrets.clone())?
                    .list_models(operation)
                    .await
            }
            NativeConnector::Ollama(config) => {
                OllamaConnector::new(&config).list_models(operation).await
            }
            NativeConnector::LmStudio(config) => {
                LmStudioConnector::new(connector_name, &config, self.secrets.clone())?
                    .list_models(operation)
                    .await
            }
            NativeConnector::Anthropic(config) => {
                AnthropicConnector::new(config, self.secrets.clone())?
                    .list_models(operation)
                    .await
            }
            NativeConnector::Codex => {
                let ConnectorConfig::CodexAppServer(config) = connector else {
                    unreachable!()
                };
                let models = CodexAppServerProvider::discover_models(config, operation).await?;
                Ok(models
                    .into_iter()
                    .map(|model| ConnectorModelSummary {
                        id: model.model,
                        display_name: model.display_name,
                        is_default: model.is_default,
                        hidden: model.hidden,
                        input_modalities: model.input_modalities,
                        output_modalities: vec!["text".into()],
                        context_length: None,
                        description: None,
                        metadata: Default::default(),
                    })
                    .collect())
            }
            NativeConnector::Generic => Err(SfumatoError::config(
                "This generic OpenAI-compatible connector does not advertise a native model catalog",
            )),
        }
    }

    async fn status(
        &self,
        connector_name: &str,
        connector: &ConnectorConfig,
        operation: &OperationContext,
    ) -> SfumatoResult<ConnectorStatus> {
        match native_connector(connector) {
            NativeConnector::OpenRouter(config) => {
                OpenRouterConnector::new(connector_name, &config, self.secrets.clone())?
                    .status(connector_name, operation)
                    .await
            }
            NativeConnector::Ollama(config) => {
                OllamaConnector::new(&config)
                    .status(connector_name, operation)
                    .await
            }
            NativeConnector::LmStudio(config) => {
                LmStudioConnector::new(connector_name, &config, self.secrets.clone())?
                    .status(connector_name, operation)
                    .await
            }
            NativeConnector::Anthropic(config) => {
                AnthropicConnector::new(config, self.secrets.clone())?
                    .status(connector_name, operation)
                    .await
            }
            NativeConnector::Codex => {
                let ConnectorConfig::CodexAppServer(config) = connector else {
                    unreachable!()
                };
                CodexAppServerProvider::discover_status(connector_name, config, operation).await
            }
            NativeConnector::Generic => Err(SfumatoError::config(
                "This generic OpenAI-compatible connector does not advertise native status operations",
            )),
        }
    }
}

enum NativeConnector {
    OpenRouter(OpenRouterConnectorConfig),
    Ollama(OllamaConnectorConfig),
    LmStudio(LmStudioConnectorConfig),
    Anthropic(AnthropicConnectorConfig),
    Codex,
    Generic,
}

fn native_connector(connector: &ConnectorConfig) -> NativeConnector {
    match connector {
        ConnectorConfig::OpenRouter(config) => NativeConnector::OpenRouter(config.clone()),
        ConnectorConfig::Ollama(config) => NativeConnector::Ollama(config.clone()),
        ConnectorConfig::LmStudio(config) => NativeConnector::LmStudio(config.clone()),
        ConnectorConfig::Anthropic(config) => NativeConnector::Anthropic(config.clone()),
        ConnectorConfig::CodexAppServer(_) => NativeConnector::Codex,
        ConnectorConfig::OpenAiCompatible(config) if config.base_url.contains("openrouter.ai") => {
            NativeConnector::OpenRouter(OpenRouterConnectorConfig {
                transport: config.clone(),
            })
        }
        ConnectorConfig::OpenAiCompatible(config) if is_local_port(&config.base_url, 11434) => {
            NativeConnector::Ollama(OllamaConnectorConfig {
                transport: config.clone(),
                native_base_url: config
                    .base_url
                    .trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .into(),
            })
        }
        ConnectorConfig::OpenAiCompatible(config) if is_local_port(&config.base_url, 1234) => {
            NativeConnector::LmStudio(LmStudioConnectorConfig {
                transport: config.clone(),
                native_base_url: config
                    .base_url
                    .trim_end_matches('/')
                    .trim_end_matches("/v1")
                    .into(),
            })
        }
        ConnectorConfig::OpenAiCompatible(_) => NativeConnector::Generic,
    }
}

/// Whether a legacy base URL unambiguously names a loopback host on `port`.
///
/// ADR-0008 only allows promoting a generic connector to a native adapter when
/// the URL identifies the provider unambiguously, so this parses the authority
/// instead of substring-matching: `contains("localhost:1234")` also matches
/// `localhost:12345`, which would misroute an unrelated local server.
pub(crate) fn is_local_port(base_url: &str, port: u16) -> bool {
    let authority = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    // Userinfo cannot appear in these URLs, but stripping it keeps the host exact.
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let Some((host, actual)) = host_port.rsplit_once(':') else {
        return false;
    };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1") && actual.parse::<u16>() == Ok(port)
}

#[cfg(test)]
#[path = "../tests/unit/providers.rs"]
mod tests;
