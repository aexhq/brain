use async_trait::async_trait;
use brain_protocol::{
    ModelBinding, ModelPresentation, ModelRequest, ModelResult, ModelStreamEvent, OperationId,
};

mod accumulator;
mod anthropic;
mod http;
mod openai;
mod registry;
mod sse;

// Exported so the performance spikes can measure the shipped decoder rather than a copy.
pub use sse::SseDecoder;

pub use accumulator::Accumulator;
pub use http::{ModelTransport, RemoteModelClient, RemoteModelConfig};
pub use registry::{Dialect, PROVIDERS, ProviderSpec, provider_spec, valid_model_name};

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
