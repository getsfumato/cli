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
