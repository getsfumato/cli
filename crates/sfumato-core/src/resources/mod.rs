use async_trait::async_trait;

use crate::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{
        ImageGenerationProvider, ImageGenerationRequest, ImageGenerationResponse,
        VideoGenerationProvider, VideoGenerationRequest, VideoGenerationResponse,
    },
};

pub mod documents;
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
