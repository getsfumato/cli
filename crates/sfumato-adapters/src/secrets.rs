//! Environment and native operating-system secret storage.

use async_trait::async_trait;
use keyring::{Entry, Error as KeyringError};
use sfumato_core::config::SecretRef;
use sfumato_core::{
    errors::{ErrorClass, SfumatoError, SfumatoResult},
    secrets::{SecretResolver, SecretStore, SecretValue},
};
use std::sync::Arc;

const KEYRING_SERVICE: &str = "sfumato";

/// Resolves environment references and stores local credentials in the OS keyring.
#[derive(Clone)]
pub struct SystemSecretStore {
    service: String,
    backend: Arc<dyn CredentialBackend>,
}

impl Default for SystemSecretStore {
    fn default() -> Self {
        Self {
            service: KEYRING_SERVICE.to_string(),
            backend: Arc::new(NativeKeyringBackend),
        }
    }
}

impl SystemSecretStore {
    fn target<'a>(&self, reference: &'a SecretRef) -> SfumatoResult<&'a str> {
        if reference.scheme() != "stored" {
            return Err(unsupported_write(reference));
        }
        Ok(reference.target())
    }
}

trait CredentialBackend: Send + Sync {
    fn get(&self, service: &str, target: &str) -> Result<String, KeyringError>;
    fn set(&self, service: &str, target: &str, value: &str) -> Result<(), KeyringError>;
    fn delete(&self, service: &str, target: &str) -> Result<(), KeyringError>;
}

struct NativeKeyringBackend;

impl NativeKeyringBackend {
    fn entry(service: &str, target: &str) -> Result<Entry, KeyringError> {
        Entry::new(service, target)
    }
}

impl CredentialBackend for NativeKeyringBackend {
    fn get(&self, service: &str, target: &str) -> Result<String, KeyringError> {
        Self::entry(service, target)?.get_password()
    }

    fn set(&self, service: &str, target: &str, value: &str) -> Result<(), KeyringError> {
        Self::entry(service, target)?.set_password(value)
    }

    fn delete(&self, service: &str, target: &str) -> Result<(), KeyringError> {
        Self::entry(service, target)?.delete_credential()
    }
}

#[async_trait]
impl SecretResolver for SystemSecretStore {
    async fn resolve(&self, reference: &SecretRef) -> SfumatoResult<SecretValue> {
        let value = match reference.scheme() {
            "env" => std::env::var(reference.target()).map_err(|_| {
                SfumatoError::config(format!(
                    "Missing credential environment variable {}",
                    reference.target()
                ))
            })?,
            "stored" => self
                .backend
                .get(&self.service, self.target(reference)?)
                .map_err(|error| match error {
                    KeyringError::NoEntry => SfumatoError::not_found(format!(
                        "Stored credential '{}' was not found. Run `sfumato connector login <connector>`.",
                        reference.target()
                    )),
                    other => keyring_error(other),
                })?,
            scheme => {
                return Err(SfumatoError::config(format!(
                    "Unsupported connector credential scheme '{scheme}'"
                )));
            }
        };
        if value.is_empty() {
            return Err(SfumatoError::validation(format!(
                "Credential '{}' is empty",
                reference.target()
            )));
        }
        Ok(SecretValue::new(value))
    }
}

#[async_trait]
impl SecretStore for SystemSecretStore {
    async fn save(&self, reference: &SecretRef, value: SecretValue) -> SfumatoResult<()> {
        if value.is_empty() {
            return Err(SfumatoError::validation("Credentials cannot be empty"));
        }
        self.backend
            .set(&self.service, self.target(reference)?, value.expose())
            .map_err(keyring_error)
    }

    async fn exists(&self, reference: &SecretRef) -> SfumatoResult<bool> {
        match reference.scheme() {
            "env" => Ok(std::env::var_os(reference.target()).is_some()),
            "stored" => match self.backend.get(&self.service, self.target(reference)?) {
                Ok(_) => Ok(true),
                Err(KeyringError::NoEntry) => Ok(false),
                Err(error) => Err(keyring_error(error)),
            },
            scheme => Err(SfumatoError::config(format!(
                "Unsupported connector credential scheme '{scheme}'"
            ))),
        }
    }

    async fn delete(&self, reference: &SecretRef) -> SfumatoResult<()> {
        match self.backend.delete(&self.service, self.target(reference)?) {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(keyring_error(error)),
        }
    }
}

fn unsupported_write(reference: &SecretRef) -> SfumatoError {
    SfumatoError::validation(format!(
        "Credential reference '{}' is read-only; secure storage requires a stored: reference",
        reference
    ))
}

fn keyring_error(error: KeyringError) -> SfumatoError {
    SfumatoError::new(
        sfumato_core::errors::ErrorCode::Config,
        ErrorClass::Unavailable,
        format!("Could not access the operating-system credential store: {error}"),
    )
}

#[cfg(test)]
#[path = "../tests/unit/secrets.rs"]
mod tests;
