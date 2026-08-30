use async_trait::async_trait;
use brain_protocol::{
    ModelBinding, ModelPresentation, ModelRequest, ModelResult, ModelStreamEvent, OperationId,
};

mod accumulator;
mod anthropic;
mod generated;
mod http;
mod openai;
mod registry;
mod sse;

// Exported so the performance spikes can measure the shipped decoder rather than a copy.
pub use sse::SseDecoder;

pub use accumulator::Accumulator;
pub use generated::{CATALOG, SNAPSHOT_DIGEST};
pub use http::{ModelTransport, RemoteModelClient, RemoteModelConfig, validate_base_url};
pub use registry::{
    Dialect, MaxTokensField, ModelCost, ModelDef, ProviderDef, ProviderRegistry, valid_model_name,
    valid_provider_name,
};

use brain_protocol::Identity;

use crate::KernelError;

#[async_trait]
pub trait ModelExecutor: Send + Sync + 'static {
    async fn execute(
        &self,
        operation_id: &OperationId,
        request_identity: &Identity,
        binding: &ModelBinding,
        presentation: &ModelPresentation,
        request: ModelRequest,
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError>;
}
