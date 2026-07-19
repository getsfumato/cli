//! Production provider routing across HTTP and local process transports.

use std::sync::Arc;

use async_trait::async_trait;
use sfumato_core::{
    config::{
        Capability, ConnectorConfig, EffectiveConfig, ModelProfile, OllamaConnectorConfig,
        OpenRouterConnectorConfig,
    },
    errors::{SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{
        ConnectorCapabilities, ConnectorCapability, ConnectorIntrospection, ConnectorModelSummary,
        ConnectorStatus, ImageGenerationProvider, ProviderFactory, TextGenerationProvider,
    },
    secrets::SecretResolver,
};

use crate::{
    codex_app_server::CodexAppServerProvider, ollama::OllamaConnector,
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
            | ConnectorConfig::Ollama(_) => self.openai_compatible.text(config, profile),
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
            | ConnectorConfig::Ollama(_) => self.openai_compatible.image(config, profile),
            ConnectorConfig::CodexAppServer(_) => Err(SfumatoError::config(
                "Codex App Server connectors support text generation only",
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
                ],
            ),
            NativeConnector::Ollama(_) => (
                "ollama",
                vec![
                    ConnectorCapability::ModelCatalog,
                    ConnectorCapability::RuntimeStatus,
                ],
            ),
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
    Codex,
    Generic,
}

fn native_connector(connector: &ConnectorConfig) -> NativeConnector {
    match connector {
        ConnectorConfig::OpenRouter(config) => NativeConnector::OpenRouter(config.clone()),
        ConnectorConfig::Ollama(config) => NativeConnector::Ollama(config.clone()),
        ConnectorConfig::CodexAppServer(_) => NativeConnector::Codex,
        ConnectorConfig::OpenAiCompatible(config) if config.base_url.contains("openrouter.ai") => {
            NativeConnector::OpenRouter(OpenRouterConnectorConfig {
                transport: config.clone(),
            })
        }
        ConnectorConfig::OpenAiCompatible(config)
            if config.base_url.contains("localhost:11434")
                || config.base_url.contains("127.0.0.1:11434") =>
        {
            NativeConnector::Ollama(OllamaConnectorConfig {
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
