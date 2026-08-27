use async_trait::async_trait;
use brain_protocol::{
    ModelBinding, ModelPresentation, ModelRequest, ModelResult, ModelStreamEvent, OperationId,
};

mod http;
mod sse;

pub use http::{RemoteModelClient, RemoteModelConfig};

use crate::KernelError;

#[async_trait]
pub trait ModelExecutor: Send + Sync + 'static {
    async fn execute(
        &self,
        operation_id: &OperationId,
        request_digest: &str,
        binding: &ModelBinding,
        presentation: &ModelPresentation,
        request: ModelRequest,
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, KernelError>;
}
