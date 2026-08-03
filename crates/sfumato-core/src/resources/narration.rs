//! Spoken narration: synthesis, timing, and caption grouping.
//!
//! Engine-neutral on purpose. A film wants the words placed against a storyboard
//! and a podcast wants them placed one after another, but both need the same
//! three things from a synthesiser — audio on disk, how long each passage
//! actually runs, and where every word falls inside it — so those live here
//! rather than inside the video workflow that happens to need them first.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    config::SpeechModelOptions,
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult as Result},
    filesystem::WorkspaceFileSystem,
    operation::OperationContext,
    providers::{SpeechGenerationProvider, SpeechGenerationRequest, SpeechWordTiming},
};

/// Silence held after a spoken passage when the profile names no gap.
///
/// Measured against how narrated explainers read rather than picked round: under
/// a third of a second the next beat cuts in on the last syllable, and past a
/// second the film feels like it stalled between sentences.
pub const DEFAULT_SEGMENT_GAP_SECONDS: f32 = 0.45;

/// Largest passage one request may speak.
///
/// Providers cap a request's characters and bill by them, so a runaway plan is
/// refused here with a message that names the passage instead of surfacing a
/// provider error nobody can attribute.
const MAX_SEGMENT_CHARACTERS: usize = 5_000;

/// Longest identifier accepted for a passage.
const MAX_SEGMENT_ID_CHARACTERS: usize = 64;

/// Rejects an identifier that is unsafe to build a file name out of.
///
/// The caller's ID becomes a path component in `narration-<id>-<digest>.<ext>`,
/// and the callers that matter are upstream of a model: a film names passages
/// after plan scenes. Video plans already pin the charset in the domain, but this
/// function is engine-neutral and a podcast naming passages after chapters gets
/// the same guarantee here rather than by convention.
fn validate_segment_id(id: &str) -> Result<()> {
    let permitted =
        |character: char| character.is_ascii_alphanumeric() || matches!(character, '-' | '_');
    if id.is_empty() || id.chars().count() > MAX_SEGMENT_ID_CHARACTERS {
        return Err(SfumatoError::validation(format!(
            "Narration passage identifier '{id}' must be between 1 and {MAX_SEGMENT_ID_CHARACTERS} characters"
        )));
    }
    if !id.chars().all(permitted) {
        return Err(SfumatoError::validation(format!(
            "Narration passage identifier '{id}' must use only letters, digits, `-`, and `_`"
        )));
    }
    Ok(())
}

/// One passage to speak, named by whatever owns it.
#[derive(Clone, Debug)]
pub struct NarrationSegmentRequest {
    /// Stable identifier — a scene ID for a film, a chapter for a podcast.
    pub id: String,
    /// Exactly the words to speak.
    pub text: String,
}

/// One spoken passage, on disk and timed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NarrationSegment {
    /// Identifier supplied by the caller.
    pub id: String,
    /// The words that were spoken.
    pub text: String,
    /// Absolute path of the written audio file.
    pub path: PathBuf,
    /// Path relative to the source root, for embedding in a composition.
    pub reference: String,
    /// Spoken length in seconds, excluding any gap after it.
    pub duration_seconds: f32,
    /// Word timings relative to this passage's own audio.
    pub words: Vec<SpeechWordTiming>,
}

/// Every spoken passage produced for one resource.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct NarrationTrack {
    /// Passages in the order they were requested.
    pub segments: Vec<NarrationSegment>,
    /// Silence held after each passage, in seconds.
    pub segment_gap_seconds: f32,
}

impl NarrationTrack {
    /// Returns the passage spoken for one identifier.
    pub fn segment(&self, id: &str) -> Option<&NarrationSegment> {
        self.segments.iter().find(|segment| segment.id == id)
    }

    /// Total spoken length including the gap held after every passage.
    pub fn total_seconds(&self) -> f32 {
        self.segments
            .iter()
            .map(|segment| segment.duration_seconds + self.segment_gap_seconds)
            .sum()
    }
}

/// Inputs for one narration synthesis pass.
pub struct SynthesizeNarrationRequest<'a> {
    /// Passages to speak, in playback order.
    pub segments: Vec<NarrationSegmentRequest>,
    /// Resolved speech provider.
    pub provider: &'a dyn SpeechGenerationProvider,
    /// Typed speech defaults from the selected profile.
    pub options: &'a SpeechModelOptions,
    /// Directory the audio files are written to.
    pub output_dir: &'a Path,
    /// Prefix prepended to each file name in `reference`.
    pub reference_prefix: &'a str,
    /// Workspace used for every write.
    pub workspace: &'a dyn WorkspaceFileSystem,
    /// Cancellation-aware operation context.
    pub operation: &'a OperationContext,
    /// Stage reported by provider errors and checkpoints.
    pub stage: OperationStage,
}

/// Speaks every passage in order and writes each one to the output directory.
///
/// Passages are spoken one at a time rather than concurrently, and each request
/// carries the neighbouring lines: a synthesiser given isolated sentences resets
/// its intonation on every one, which is what makes stitched narration sound
/// like a list being read out.
pub async fn synthesize_narration(
    request: SynthesizeNarrationRequest<'_>,
) -> Result<NarrationTrack> {
    let SynthesizeNarrationRequest {
        segments,
        provider,
        options,
        output_dir,
        reference_prefix,
        workspace,
        operation,
        stage,
    } = request;
    let spoken = segments
        .into_iter()
        .filter(|segment| !segment.text.trim().is_empty())
        .collect::<Vec<_>>();
    for segment in &spoken {
        validate_segment_id(&segment.id)?;
        if segment.text.chars().count() > MAX_SEGMENT_CHARACTERS {
            return Err(SfumatoError::validation(format!(
                "Narration for '{}' is {} characters; the per-request limit is {MAX_SEGMENT_CHARACTERS}",
                segment.id,
                segment.text.chars().count()
            )));
        }
    }
    let mut track = NarrationTrack {
        segments: Vec::with_capacity(spoken.len()),
        segment_gap_seconds: options
            .segment_gap_seconds
            .filter(|gap| gap.is_finite() && *gap >= 0.0)
            .unwrap_or(DEFAULT_SEGMENT_GAP_SECONDS),
    };
    let extension = audio_extension(options.output_format.as_deref());
    workspace.create_dir_all(output_dir)?;
    for (index, segment) in spoken.iter().enumerate() {
        operation.checkpoint(stage)?;
        let response = provider
            .generate_speech(
                SpeechGenerationRequest {
                    text: segment.text.clone(),
                    voice: options.voice.clone(),
                    previous_text: index
                        .checked_sub(1)
                        .and_then(|previous| spoken.get(previous))
                        .map(|previous| previous.text.clone()),
                    next_text: spoken.get(index + 1).map(|next| next.text.clone()),
                },
                operation,
                stage,
            )
            .await?;
        if response.bytes.is_empty() {
            return Err(SfumatoError::provider(
                ErrorClass::InvalidOutput,
                format!("Speech provider returned no audio for '{}'", segment.id),
            ));
        }
        // The provider's alignment is the only source that agrees with the
        // captions; a duration guessed from the text would drift a word at a
        // time and land the last caption over the next scene.
        let duration_seconds = response
            .duration_seconds
            .or_else(|| response.words.last().map(|word| word.end_seconds))
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| {
                SfumatoError::provider(
                    ErrorClass::InvalidOutput,
                    format!(
                        "Speech provider returned no timing for '{}', so its narration cannot be synchronised",
                        segment.id
                    ),
                )
            })?;
        let digest = format!("{:x}", Sha256::digest(&response.bytes));
        let filename = format!("narration-{}-{}.{extension}", segment.id, &digest[..12]);
        let path = output_dir.join(&filename);
        workspace.write(&path, &response.bytes)?;
        track.segments.push(NarrationSegment {
            id: segment.id.clone(),
            text: segment.text.clone(),
            path,
            reference: format!("{}/{filename}", reference_prefix.trim_end_matches('/')),
            duration_seconds,
            words: response.words,
        });
    }
    Ok(track)
}

/// File extension implied by a provider output format such as `mp3_44100_128`.
pub fn audio_extension(output_format: Option<&str>) -> &'static str {
    match output_format
        .unwrap_or("mp3")
        .split('_')
        .next()
        .unwrap_or("mp3")
    {
        "wav" => "wav",
        "pcm" => "pcm",
        "opus" => "opus",
        "ulaw" | "alaw" => "wav",
        _ => "mp3",
    }
}

/// Media type implied by a provider output format.
pub fn audio_media_type(output_format: Option<&str>) -> &'static str {
    match audio_extension(output_format) {
        "wav" => "audio/wav",
        "opus" => "audio/ogg",
        "pcm" => "audio/basic",
        _ => "audio/mpeg",
    }
}

/// A run of words shown together as one caption.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CaptionGroup {
    /// Words joined with single spaces.
    pub text: String,
    /// Start on the film's own timeline, in seconds.
    pub start_seconds: f32,
    /// End on the film's own timeline, in seconds.
    pub end_seconds: f32,
    /// The words themselves, also on the film's timeline.
    pub words: Vec<SpeechWordTiming>,
}

/// How many words one caption may hold before it is broken.
///
/// Five is the conversational grouping the caption doctrine calls for: enough to
/// read as a phrase, few enough that a viewer never re-reads a line.
const MAX_CAPTION_WORDS: usize = 5;

/// Silence after which a caption is broken even below the word limit.
const CAPTION_PAUSE_SECONDS: f32 = 0.35;

/// Groups one passage's words into captions, offset onto the film's timeline.
///
/// Breaks on sentence punctuation, on a real pause, and at the word cap, in that
/// order of authority: a group that runs across a full stop reads as two
/// sentences fused, which is worse than a short line.
pub fn caption_groups(words: &[SpeechWordTiming], offset_seconds: f32) -> Vec<CaptionGroup> {
    let mut groups: Vec<CaptionGroup> = Vec::new();
    let mut current: Vec<SpeechWordTiming> = Vec::new();
    let mut previous_end: Option<f32> = None;
    for word in words {
        let shifted = SpeechWordTiming {
            text: word.text.clone(),
            start_seconds: word.start_seconds + offset_seconds,
            end_seconds: word.end_seconds + offset_seconds,
        };
        let paused =
            previous_end.is_some_and(|end| shifted.start_seconds - end >= CAPTION_PAUSE_SECONDS);
        if !current.is_empty() && (paused || current.len() >= MAX_CAPTION_WORDS) {
            groups.push(finish_group(std::mem::take(&mut current)));
        }
        let ends_sentence = shifted.text.ends_with(['.', '!', '?', '…', ':', ';']);
        previous_end = Some(shifted.end_seconds);
        current.push(shifted);
        if ends_sentence {
            groups.push(finish_group(std::mem::take(&mut current)));
        }
    }
    if !current.is_empty() {
        groups.push(finish_group(current));
    }
    groups
}

fn finish_group(words: Vec<SpeechWordTiming>) -> CaptionGroup {
    let start_seconds = words.first().map(|word| word.start_seconds).unwrap_or(0.0);
    let end_seconds = words.last().map(|word| word.end_seconds).unwrap_or(0.0);
    CaptionGroup {
        text: words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        start_seconds,
        end_seconds,
        words,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/resources_narration.rs"]
mod tests;
