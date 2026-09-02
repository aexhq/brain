use async_trait::async_trait;
use brain_protocol::{ModelBinding, ModelRequest, ModelResult, ModelStreamEvent, ToolDefinition};

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

use crate::Error;

#[async_trait]
pub trait ModelExecutor: Send + Sync + 'static {
    /// Makes one model call. `tools` are the definitions of the tools the request names,
    /// resolved by the session from what it was created with, in the request's order.
    async fn execute(
        &self,
        binding: &ModelBinding,
        request: ModelRequest,
        tools: &[ToolDefinition],
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, Error>;
}
