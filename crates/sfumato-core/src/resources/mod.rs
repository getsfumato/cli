use async_trait::async_trait;

use crate::{
    errors::{ErrorClass, OperationStage, SfumatoError, SfumatoResult},
    operation::OperationContext,
    providers::{ImageGenerationProvider, ImageGenerationRequest, ImageGenerationResponse},
};

pub mod pages;
pub(crate) mod project_assets;
pub mod slides;

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
