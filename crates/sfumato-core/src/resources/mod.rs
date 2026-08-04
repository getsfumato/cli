use async_trait::async_trait;

use crate::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{
        ImageGenerationProvider, ImageGenerationRequest, ImageGenerationResponse,
        SpeechGenerationProvider, SpeechGenerationRequest, SpeechGenerationResponse,
        VideoGenerationProvider, VideoGenerationRequest, VideoGenerationResponse,
    },
};

pub mod documents;
pub mod narration;
pub mod pages;
pub(crate) mod project_assets;
pub mod slides;
pub mod videos;

/// Truncates text destined for a prompt, saying so when it truncates.
///
/// The marker is the point. Two of the copies this replaces cut silently, and
/// both fed repair prompts — the invalid response handed back for correction, and
/// the draft passed into validation repair. A model asked to repair a document
/// whose tail was removed without a word is liable to "fix" a structure that only
/// looks unterminated: re-closing sections that were fine, or regenerating a tail
/// that already existed.
///
/// Naming sfumato in the marker matters too: it tells the model the cut is the
/// tool's doing and not something to repair.
pub(crate) fn excerpt(content: &str, max_chars: usize) -> String {
    let mut excerpt = content.chars().take(max_chars).collect::<String>();
    if content.chars().count() > max_chars {
        excerpt.push_str("\n[...truncated by sfumato...]");
    }
    excerpt
}

pub(crate) struct DryRunImageProvider;

#[async_trait]
impl ImageGenerationProvider for DryRunImageProvider {
    async fn generate_image(
        &self,
        _request: ImageGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> SfumatoResult<ImageGenerationResponse> {
        Err(SfumatoError::provider(
            ErrorClass::Unavailable,
            "Dry-run image provider cannot execute",
        ))
    }
}

pub(crate) struct DryRunVideoProvider;

#[async_trait]
impl VideoGenerationProvider for DryRunVideoProvider {
    async fn generate_video(
        &self,
        _request: VideoGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> SfumatoResult<VideoGenerationResponse> {
        Err(SfumatoError::provider(
            ErrorClass::Unavailable,
            "Dry-run video provider cannot execute",
        ))
    }
}

pub(crate) struct DryRunSpeechProvider;

#[async_trait]
impl SpeechGenerationProvider for DryRunSpeechProvider {
    async fn generate_speech(
        &self,
        _request: SpeechGenerationRequest,
        _operation: &OperationContext,
        _stage: OperationStage,
    ) -> SfumatoResult<SpeechGenerationResponse> {
        Err(SfumatoError::provider(
            ErrorClass::Unavailable,
            "Dry-run speech provider cannot execute",
        ))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/resources.rs"]
mod tests;
