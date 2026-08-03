//! Native ElevenLabs speech synthesis, voice catalog, and account status.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use sfumato_core::{
    config::{ElevenLabsConnectorConfig, ModelProfile, SpeechModelOptions},
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{
        ConnectorModelSummary, ConnectorStatus, ConnectorStatusField, SpeechGenerationProvider,
        SpeechGenerationRequest, SpeechGenerationResponse, SpeechWordTiming,
    },
    retry::RETRY_AFTER_DETAIL,
    secrets::SecretResolver,
};

use crate::runtime::await_operation;

/// Synthesis is a single blocking call whose length tracks the passage, so this
/// is generous compared with a chat turn and still bounded.
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

/// Container ElevenLabs returns when a profile names none.
const DEFAULT_OUTPUT_FORMAT: &str = "mp3_44100_128";

/// Largest response body accepted, guarding against a mis-set output format.
const MAX_AUDIO_BYTES: usize = 64 * 1024 * 1024;

/// ElevenLabs connector over its native `xi-api-key` HTTP API.
#[derive(Clone)]
pub struct ElevenLabsConnector {
    client: Client,
    config: ElevenLabsConnectorConfig,
    secrets: Arc<dyn SecretResolver>,
}

impl ElevenLabsConnector {
    /// Creates a connector that resolves its credential at request time.
    pub fn new(
        config: ElevenLabsConnectorConfig,
        secrets: Arc<dyn SecretResolver>,
    ) -> SfumatoResult<Self> {
        Client::builder()
            .timeout(HTTP_REQUEST_TIMEOUT)
            .build()
            .map(|client| Self {
                client,
                config,
                secrets,
            })
            .map_err(|error| {
                SfumatoError::config(format_args!(
                    "Could not build the ElevenLabs HTTP client: {error}"
                ))
            })
    }

    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn authorized(&self, request: RequestBuilder) -> Result<RequestBuilder> {
        let mut request = request;
        if let Some(reference) = &self.config.credential {
            let key = self.secrets.resolve(reference).await?;
            // Not bearer auth: ElevenLabs reads the key from its own header, and
            // an `Authorization` header is ignored rather than rejected, which
            // would surface as an unauthenticated call nobody could explain.
            request = request.header("xi-api-key", key.expose());
        }
        for (name, value) in &self.config.headers {
            request = request.header(name, value);
        }
        Ok(request)
    }

    async fn get(&self, path: &str) -> Result<RequestBuilder> {
        self.authorized(self.client.get(self.endpoint(path))).await
    }

    async fn post(&self, path: &str) -> Result<RequestBuilder> {
        self.authorized(self.client.post(self.endpoint(path))).await
    }

    /// Creates a speech provider bound to one resolved model profile.
    pub fn speech_provider(&self, profile: ModelProfile) -> ElevenLabsSpeechProvider {
        ElevenLabsSpeechProvider {
            connector: self.clone(),
            profile,
        }
    }

    /// Lists the synthesis models this account may use.
    pub async fn list_models(
        &self,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<ConnectorModelSummary>> {
        native_result(
            async {
                let body = self.read("v1/models", operation, "model catalog").await?;
                let models: Vec<ElevenLabsModel> = serde_json::from_str(&body)
                    .context("ElevenLabs model catalog returned invalid JSON")?;
                Ok(models.into_iter().map(map_model).collect())
            }
            .await,
        )
    }

    /// Lists the voices this account may speak with.
    ///
    /// Presented as models because a voice is the other half of a speech
    /// profile's identity, and a caller choosing one needs the same listing
    /// surface. Every entry names the option it fills in, so the two cannot be
    /// mistaken for each other in `sfumato connector models`.
    pub async fn list_voices(
        &self,
        operation: &OperationContext,
    ) -> SfumatoResult<Vec<ConnectorModelSummary>> {
        native_result(
            async {
                let body = self
                    .read("v2/voices?page_size=100", operation, "voice catalog")
                    .await?;
                let voices: VoicesResponse = serde_json::from_str(&body)
                    .context("ElevenLabs voice catalog returned invalid JSON")?;
                Ok(voices.voices.into_iter().map(map_voice).collect())
            }
            .await,
        )
    }

    /// Reads subscription tier and character usage for the configured key.
    pub async fn status(
        &self,
        name: &str,
        operation: &OperationContext,
    ) -> SfumatoResult<ConnectorStatus> {
        native_result(
            async {
                let body = self
                    .read("v1/user/subscription", operation, "subscription")
                    .await?;
                let subscription: Subscription = serde_json::from_str(&body)
                    .context("ElevenLabs subscription returned invalid JSON")?;
                let mut fields = vec![
                    field(
                        "tier",
                        subscription.tier.unwrap_or_else(|| "unknown".into()),
                    ),
                    field(
                        "status",
                        subscription.status.unwrap_or_else(|| "unknown".into()),
                    ),
                ];
                if let Some(used) = subscription.character_count {
                    fields.push(field("characters_used", used));
                }
                if let Some(limit) = subscription.character_limit {
                    fields.push(field("character_limit", limit));
                    if let Some(used) = subscription.character_count {
                        fields.push(field("characters_remaining", limit.saturating_sub(used)));
                    }
                }
                if let Some(reset) = subscription.next_character_count_reset_unix {
                    fields.push(field("next_reset_unix", reset));
                }
                if let Some(limit) = subscription.voice_limit {
                    fields.push(field("voice_limit", limit));
                }
                Ok(ConnectorStatus {
                    connector: name.into(),
                    kind: "elevenlabs".into(),
                    fields,
                })
            }
            .await,
        )
    }

    async fn read(&self, path: &str, operation: &OperationContext, what: &str) -> Result<String> {
        // Every native read is a resolve-stage call: it answers a question about
        // the connector rather than producing part of a resource.
        let request = self.get(path).await?.send();
        let response = await_operation(operation, OperationStage::Resolve, request).await?;
        let status = response.status();
        let retry_after = retry_after_header(response.headers());
        let body = await_operation(operation, OperationStage::Resolve, response.text()).await?;
        if !status.is_success() {
            return Err(classify_status(what, status, &body, retry_after.as_deref())
                .at_stage(OperationStage::Resolve)
                .into());
        }
        Ok(body)
    }
}

/// Speech provider for one ElevenLabs model profile.
pub struct ElevenLabsSpeechProvider {
    connector: ElevenLabsConnector,
    profile: ModelProfile,
}

#[async_trait]
impl SpeechGenerationProvider for ElevenLabsSpeechProvider {
    async fn generate_speech(
        &self,
        request: SpeechGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> SfumatoResult<SpeechGenerationResponse> {
        let result = self.generate(request, operation, stage).await;
        result.map_err(|error| {
            if let Some(error) = error.downcast_ref::<SfumatoError>() {
                return error.clone();
            }
            SfumatoError::provider(ErrorClass::Unavailable, format_args!("{error:#}"))
                .at_stage(stage)
        })
    }
}

impl ElevenLabsSpeechProvider {
    async fn generate(
        &self,
        request: SpeechGenerationRequest,
        operation: &OperationContext,
        stage: OperationStage,
    ) -> Result<SpeechGenerationResponse> {
        let options = &self.profile.options.speech;
        let voice = request
            .voice
            .as_deref()
            .or(options.voice.as_deref())
            .context(
                "This ElevenLabs profile names no voice. Set `speech_voice` on the model profile or pick one with `sfumato connector models <connector>`.",
            )?;
        let output_format = options
            .output_format
            .as_deref()
            .unwrap_or(DEFAULT_OUTPUT_FORMAT);
        let payload = SpeechRequestBody {
            text: request.text,
            model_id: self.profile.model.clone(),
            language_code: options.language.clone(),
            previous_text: request.previous_text,
            next_text: request.next_text,
            voice_settings: voice_settings(options),
        };
        // The timestamped endpoint rather than the plain one: word alignment is
        // what the captions and the film's own timeline are built from, and it
        // is not recoverable from the audio afterwards.
        let send = self
            .connector
            .post(&format!(
                "v1/text-to-speech/{voice}/with-timestamps?output_format={output_format}"
            ))
            .await?
            .json(&payload)
            .send();
        let response = await_operation(operation, stage, send).await?;
        let status = response.status();
        let retry_after = retry_after_header(response.headers());
        let body = await_operation(operation, stage, response.text()).await?;
        if !status.is_success() {
            return Err(
                classify_status("speech synthesis", status, &body, retry_after.as_deref())
                    .at_stage(stage)
                    .into(),
            );
        }
        let synthesized: SpeechResponseBody =
            serde_json::from_str(&body).context("ElevenLabs speech returned invalid JSON")?;
        let bytes = STANDARD
            .decode(synthesized.audio_base64.trim())
            .context("ElevenLabs speech returned audio that is not valid base64")?;
        if bytes.is_empty() {
            bail!("ElevenLabs speech returned an empty audio file");
        }
        if bytes.len() > MAX_AUDIO_BYTES {
            bail!(
                "ElevenLabs speech returned {} bytes; the current limit is {MAX_AUDIO_BYTES}",
                bytes.len()
            );
        }
        // The normalized alignment describes the text that was actually spoken
        // after number and abbreviation expansion, which is the only version a
        // caption can match; the raw alignment describes the text as written.
        let alignment = synthesized
            .normalized_alignment
            .or(synthesized.alignment)
            .unwrap_or_default();
        let words = words_from_alignment(&alignment);
        Ok(SpeechGenerationResponse {
            duration_seconds: alignment
                .character_end_times_seconds
                .last()
                .copied()
                .or_else(|| words.last().map(|word| word.end_seconds)),
            words,
            media_type: media_type(output_format).to_string(),
            bytes,
        })
    }
}

/// Groups a character-level alignment into words.
///
/// ElevenLabs times individual characters, so a word's window runs from its
/// first character's start to its last character's end. Whitespace closes a
/// word and belongs to neither side, which keeps a caption from lighting up
/// before its first letter is spoken.
pub(crate) fn words_from_alignment(alignment: &Alignment) -> Vec<SpeechWordTiming> {
    let mut words = Vec::new();
    let mut text = String::new();
    let mut start = 0.0_f32;
    let mut end = 0.0_f32;
    for (index, character) in alignment.characters.iter().enumerate() {
        let (Some(character_start), Some(character_end)) = (
            alignment.character_start_times_seconds.get(index).copied(),
            alignment.character_end_times_seconds.get(index).copied(),
        ) else {
            break;
        };
        if character.chars().all(char::is_whitespace) {
            if !text.is_empty() {
                words.push(SpeechWordTiming {
                    text: std::mem::take(&mut text),
                    start_seconds: start,
                    end_seconds: end,
                });
            }
            continue;
        }
        if text.is_empty() {
            start = character_start;
        }
        end = character_end;
        text.push_str(character);
    }
    if !text.is_empty() {
        words.push(SpeechWordTiming {
            text,
            start_seconds: start,
            end_seconds: end,
        });
    }
    words
}

fn voice_settings(options: &SpeechModelOptions) -> Option<VoiceSettings> {
    let settings = VoiceSettings {
        stability: options.stability,
        similarity_boost: options.similarity_boost,
        style: options.style,
        speed: options.speed,
        use_speaker_boost: options.speaker_boost,
    };
    // Omitted entirely when a profile sets nothing, so the voice keeps whatever
    // settings it was saved with rather than being reset to provider defaults.
    if settings.is_empty() {
        None
    } else {
        Some(settings)
    }
}

fn media_type(output_format: &str) -> &'static str {
    match output_format.split('_').next().unwrap_or("mp3") {
        "wav" | "ulaw" | "alaw" => "audio/wav",
        "opus" => "audio/ogg",
        "pcm" => "audio/basic",
        _ => "audio/mpeg",
    }
}

#[derive(Serialize)]
struct SpeechRequestBody {
    text: String,
    model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    language_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    voice_settings: Option<VoiceSettings>,
}

#[derive(Serialize)]
pub(crate) struct VoiceSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    stability: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    similarity_boost: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    use_speaker_boost: Option<bool>,
}

impl VoiceSettings {
    fn is_empty(&self) -> bool {
        self.stability.is_none()
            && self.similarity_boost.is_none()
            && self.style.is_none()
            && self.speed.is_none()
            && self.use_speaker_boost.is_none()
    }
}

#[derive(Deserialize)]
struct SpeechResponseBody {
    audio_base64: String,
    #[serde(default)]
    alignment: Option<Alignment>,
    #[serde(default)]
    normalized_alignment: Option<Alignment>,
}

/// Character-level timings returned beside synthesized audio.
#[derive(Default, Deserialize)]
pub(crate) struct Alignment {
    #[serde(default)]
    pub characters: Vec<String>,
    #[serde(default)]
    pub character_start_times_seconds: Vec<f32>,
    #[serde(default)]
    pub character_end_times_seconds: Vec<f32>,
}

#[derive(Deserialize)]
struct ElevenLabsModel {
    model_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    can_do_text_to_speech: bool,
    #[serde(default)]
    languages: Vec<ModelLanguage>,
    #[serde(default)]
    maximum_text_length_per_request: Option<u64>,
}

#[derive(Deserialize)]
struct ModelLanguage {
    #[serde(default)]
    language_id: Option<String>,
}

fn map_model(model: ElevenLabsModel) -> ConnectorModelSummary {
    let mut metadata = BTreeMap::from([("selects".into(), "model".to_string())]);
    if let Some(limit) = model.maximum_text_length_per_request {
        metadata.insert("max_characters".into(), limit.to_string());
    }
    let languages = model
        .languages
        .iter()
        .filter_map(|language| language.language_id.clone())
        .collect::<Vec<_>>();
    if !languages.is_empty() {
        metadata.insert("languages".into(), languages.join(", "));
    }
    ConnectorModelSummary {
        // Labelled in the name because the two halves of a speech profile share
        // one listing, and a bare row gives a caller no way to tell an
        // `--id` from a `speech_voice`.
        display_name: format!(
            "model · {}",
            model.name.clone().unwrap_or_else(|| model.model_id.clone())
        ),
        id: model.model_id,
        is_default: false,
        // A model that cannot speak is still listed, because a profile that
        // names one should fail with "this model does not do text to speech"
        // rather than "unknown model".
        hidden: !model.can_do_text_to_speech,
        input_modalities: vec!["text".into()],
        output_modalities: vec!["audio".into()],
        context_length: None,
        description: model.description,
        metadata,
    }
}

#[derive(Deserialize)]
struct VoicesResponse {
    #[serde(default)]
    voices: Vec<ElevenLabsVoice>,
}

#[derive(Deserialize)]
struct ElevenLabsVoice {
    voice_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
}

fn map_voice(voice: ElevenLabsVoice) -> ConnectorModelSummary {
    let mut metadata = BTreeMap::from([("selects".into(), "voice".to_string())]);
    if let Some(category) = voice.category {
        metadata.insert("category".into(), category);
    }
    metadata.extend(voice.labels);
    ConnectorModelSummary {
        display_name: format!(
            "voice · {}",
            voice.name.clone().unwrap_or_else(|| voice.voice_id.clone())
        ),
        id: voice.voice_id,
        is_default: false,
        hidden: false,
        input_modalities: vec!["text".into()],
        output_modalities: vec!["audio".into()],
        context_length: None,
        description: voice.description,
        metadata,
    }
}

#[derive(Deserialize)]
struct Subscription {
    #[serde(default)]
    tier: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    character_count: Option<u64>,
    #[serde(default)]
    character_limit: Option<u64>,
    #[serde(default)]
    next_character_count_reset_unix: Option<i64>,
    #[serde(default)]
    voice_limit: Option<u64>,
}

fn field(name: &str, value: impl ToString) -> ConnectorStatusField {
    ConnectorStatusField {
        name: name.into(),
        value: value.to_string(),
    }
}

/// Reads a `Retry-After` delay so the backoff can honour the provider's own number.
fn retry_after_header(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

/// Maps an unsuccessful ElevenLabs response onto a typed recovery class.
///
/// Needed once the speech transport retries: every failure here used to collapse
/// into `Unavailable`, and under a retry that would resend a rejected credential
/// or a malformed voice ID three more times before reporting the same thing.
fn classify_status(
    what: &str,
    status: StatusCode,
    body: &str,
    retry_after: Option<&str>,
) -> SfumatoError {
    let detail = compact_detail(body);
    let error = match status.as_u16() {
        401 | 403 => SfumatoError::provider(
            ErrorClass::Permanent,
            format_args!(
                "ElevenLabs rejected the credential for {what}: {detail}. Run `sfumato connector login <name>`."
            ),
        ),
        // 422 is how ElevenLabs reports an unusable request — an unknown voice, an
        // invalid output format. Resending it changes nothing.
        400 | 404 | 422 => SfumatoError::provider(
            ErrorClass::Permanent,
            format_args!("ElevenLabs rejected {what}: {detail}"),
        ),
        408 | 429 => SfumatoError::provider(
            ErrorClass::Retry,
            format_args!("ElevenLabs rate limit reached for {what}: {detail}"),
        ),
        500..=504 | 529 => SfumatoError::provider(
            ErrorClass::Unavailable,
            format_args!("ElevenLabs is unavailable for {what}: {detail}"),
        ),
        _ => SfumatoError::provider(
            ErrorClass::Permanent,
            format_args!("ElevenLabs {what} returned HTTP {status}: {detail}"),
        ),
    };
    match retry_after {
        Some(delay) => error.with_detail(RETRY_AFTER_DETAIL, delay.to_string()),
        None => error,
    }
}

/// Trims a response body to a length that is safe to show in one error line.
fn compact_detail(body: &str) -> String {
    const MAX: usize = 400;
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    match compact.char_indices().nth(MAX) {
        Some((index, _)) => format!("{}…", &compact[..index]),
        None => compact,
    }
}

fn native_result<T>(result: Result<T>) -> SfumatoResult<T> {
    result.map_err(|error| {
        if let Some(error) = error.downcast_ref::<SfumatoError>() {
            return error.clone();
        }
        SfumatoError::provider(ErrorClass::Unavailable, format_args!("{error:#}"))
            .at_stage(OperationStage::Resolve)
    })
}

#[cfg(test)]
#[path = "../tests/unit/elevenlabs.rs"]
mod tests;
