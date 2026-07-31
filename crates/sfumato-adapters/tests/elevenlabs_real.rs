//! Speaks one short line through the configured ElevenLabs connector.
//!
//! Ignored by default: it spends the account's character budget and needs a
//! credential in the operating-system keyring. It exists because the wire
//! format — the timestamped endpoint, the base64 audio, and the character
//! alignment the captions are built from — cannot be proven by a unit test.
//!
//! Run with: `cargo test -p sfumato-adapters --test elevenlabs_real -- --ignored`

use std::{collections::BTreeMap, sync::Arc};

use sfumato_adapters::{elevenlabs::ElevenLabsConnector, secrets::SystemSecretStore};
use sfumato_core::{
    config::{Capability, ElevenLabsConnectorConfig, ModelOptions, ModelProfile, SecretRef},
    errors::OperationStage,
    operation::OperationContext,
    providers::{SpeechGenerationProvider, SpeechGenerationRequest},
};

fn profile(voice: &str) -> ModelProfile {
    let mut options = ModelOptions::default();
    options.speech.voice = Some(voice.to_string());
    options.speech.output_format = Some("mp3_44100_128".to_string());
    ModelProfile {
        connector: "elevenlabs".to_string(),
        model: "eleven_multilingual_v2".to_string(),
        capabilities: vec![Capability::Speech],
        options,
    }
}

#[tokio::test]
#[ignore = "spends the ElevenLabs character budget and needs a stored credential"]
async fn a_spoken_line_returns_audio_and_word_timings() {
    let voice = std::env::var("SFUMATO_ELEVENLABS_VOICE")
        .expect("set SFUMATO_ELEVENLABS_VOICE to a voice id from `sfumato connector models`");
    let secrets = Arc::new(SystemSecretStore::default());
    let connector = ElevenLabsConnector::new(
        ElevenLabsConnectorConfig {
            base_url: "https://api.elevenlabs.io".to_string(),
            credential: Some(SecretRef::stored("connector/elevenlabs").unwrap()),
            headers: BTreeMap::new(),
        },
        secrets,
    )
    .expect("connector builds");

    let response = connector
        .speech_provider(profile(&voice))
        .generate_speech(
            SpeechGenerationRequest {
                text: "La luz rebota dentro de la fibra.".to_string(),
                ..SpeechGenerationRequest::default()
            },
            &OperationContext::detached(),
            OperationStage::Draft,
        )
        .await
        .expect("the line is spoken");

    assert!(!response.bytes.is_empty(), "audio came back");
    assert_eq!(response.media_type, "audio/mpeg");
    let duration = response.duration_seconds.expect("the provider timed it");
    assert!(
        (0.5..10.0).contains(&duration),
        "a seven-word line runs seconds, not {duration}"
    );
    let words = response.words;
    assert!(words.len() >= 6, "one timing per word: {words:?}");
    assert_eq!(words[0].text, "La");
    assert!(words[0].start_seconds < words[0].end_seconds);
    assert!(
        words
            .windows(2)
            .all(|pair| pair[0].end_seconds <= pair[1].end_seconds),
        "word timings run forward: {words:?}"
    );
    assert!(
        words
            .last()
            .is_some_and(|word| word.end_seconds <= duration + 0.01),
        "no word ends after the audio does"
    );
}
