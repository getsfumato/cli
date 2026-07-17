use super::*;
use std::{collections::BTreeMap, sync::Mutex};

#[derive(Default)]
struct MemoryBackend(Mutex<BTreeMap<(String, String), String>>);

impl CredentialBackend for MemoryBackend {
    fn get(&self, service: &str, target: &str) -> Result<String, KeyringError> {
        self.0
            .lock()
            .unwrap()
            .get(&(service.to_string(), target.to_string()))
            .cloned()
            .ok_or(KeyringError::NoEntry)
    }

    fn set(&self, service: &str, target: &str, value: &str) -> Result<(), KeyringError> {
        self.0
            .lock()
            .unwrap()
            .insert((service.to_string(), target.to_string()), value.to_string());
        Ok(())
    }

    fn delete(&self, service: &str, target: &str) -> Result<(), KeyringError> {
        self.0
            .lock()
            .unwrap()
            .remove(&(service.to_string(), target.to_string()))
            .map(|_| ())
            .ok_or(KeyringError::NoEntry)
    }
}

fn store() -> SystemSecretStore {
    SystemSecretStore {
        service: "sfumato-test".to_string(),
        backend: Arc::new(MemoryBackend::default()),
    }
}

#[tokio::test]
async fn stores_resolves_and_deletes_protected_credentials() {
    let store = store();
    let reference = SecretRef::stored("connector/openrouter").unwrap();

    assert!(!store.exists(&reference).await.unwrap());
    store
        .save(&reference, SecretValue::new("secret-value".to_string()))
        .await
        .unwrap();
    assert!(store.exists(&reference).await.unwrap());
    assert_eq!(
        store.resolve(&reference).await.unwrap().expose(),
        "secret-value"
    );

    store.delete(&reference).await.unwrap();
    assert!(!store.exists(&reference).await.unwrap());
}

#[tokio::test]
async fn rejects_writes_to_environment_references() {
    let store = store();
    let reference = SecretRef::environment("OPENROUTER_API_KEY").unwrap();
    let error = store
        .save(&reference, SecretValue::new("secret-value".to_string()))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("read-only"));
}

#[tokio::test]
#[ignore = "touches the native operating-system credential store"]
async fn native_keyring_round_trip() {
    let store = SystemSecretStore::default();
    let reference = SecretRef::stored(&format!("test/native-{}", std::process::id())).unwrap();

    store
        .save(&reference, SecretValue::new("temporary-secret".to_string()))
        .await
        .unwrap();
    assert_eq!(
        store.resolve(&reference).await.unwrap().expose(),
        "temporary-secret"
    );
    store.delete(&reference).await.unwrap();
    assert!(!store.exists(&reference).await.unwrap());
}
