//! Protected secret values and storage ports.

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use sfumato_domain::SecretRef;

use crate::errors::SfumatoResult;

/// A secret string that is redacted from debug and display output and zeroized on drop.
pub struct SecretValue(SecretString);

impl SecretValue {
    /// Wraps a newly acquired plaintext secret.
    pub fn new(value: String) -> Self {
        Self(SecretString::from(value))
    }

    /// Exposes the plaintext only at the infrastructure boundary that needs it.
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    /// Returns whether the protected value contains no characters.
    pub fn is_empty(&self) -> bool {
        self.expose().is_empty()
    }
}

/// Read-only secret resolution used by provider infrastructure.
#[async_trait]
pub trait SecretResolver: Send + Sync {
    /// Resolves an indirect credential reference into a protected value.
    async fn resolve(&self, reference: &SecretRef) -> SfumatoResult<SecretValue>;
}

/// Secure credential management used by connector authentication workflows.
#[async_trait]
pub trait SecretStore: SecretResolver {
    /// Creates or replaces a securely stored credential.
    async fn save(&self, reference: &SecretRef, value: SecretValue) -> SfumatoResult<()>;

    /// Reports whether a credential can currently be resolved.
    async fn exists(&self, reference: &SecretRef) -> SfumatoResult<bool>;

    /// Deletes a securely stored credential.
    async fn delete(&self, reference: &SecretRef) -> SfumatoResult<()>;
}
