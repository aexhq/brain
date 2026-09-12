use async_trait::async_trait;
use brain_protocol::{
    ModelBinding, ModelRequest, ModelResult, ModelStreamEvent, SessionId, ToolDefinition,
};

mod accumulator;
mod anthropic;
#[cfg(test)]
mod catalog_tests;
#[cfg(test)]
mod continuation_tests;
mod generated;
mod http;
mod registry;
mod responses;
mod sse;

// Exported so the performance spikes can measure the shipped decoder rather than a copy.
pub use sse::SseDecoder;

pub use accumulator::Accumulator;
pub use generated::{CATALOG, SNAPSHOT_DIGEST};
pub use http::{ModelTransport, RemoteModelClient, RemoteModelConfig, validate_base_url};
pub use registry::{
    Dialect, ModelCost, ModelDef, ProviderDef, ProviderRegistry, valid_model_name,
    valid_provider_name,
};

use crate::Error;

fn options(
    body: &mut serde_json::Value,
    options: &std::collections::BTreeMap<String, serde_json::Value>,
    allowed: &[&str],
) -> Result<(), Error> {
    for (key, value) in options {
        if !allowed.contains(&key.as_str()) {
            return Err(Error::InvalidState(format!(
                "unsupported model option `{key}`"
            )));
        }
        body[key] = value.clone();
    }
    Ok(())
}

fn validate_media(url: &str) -> Result<(), Error> {
    brain_protocol::validate_media_url(url).map_err(|message| Error::InvalidState(message.into()))
}

/// Uses the actual adapter encoder to check retained context without dispatching a model call.
pub fn validate_context(
    dialect: Dialect,
    messages: Vec<brain_protocol::Message>,
) -> Result<(), Error> {
    let request = ModelRequest {
        messages,
        system: None,
        tools: None,
        response_format: None,
        max_output_tokens: None,
        options: Default::default(),
    };
    match dialect {
        Dialect::OpenAiResponses => responses::body("check", &[], &request),
        Dialect::AnthropicMessages => anthropic::body("check", &[], &request),
    }
    .map(|_| ())
}

#[async_trait]
pub trait ModelExecutor: Send + Sync + 'static {
    /// Makes one model call for `session`, whose credential the executor holds. `tools`
    /// are the definitions of the tools the request names, resolved by the session from
    /// what it was created with, in the request's order.
    async fn execute(
        &self,
        session: &SessionId,
        binding: &ModelBinding,
        request: ModelRequest,
        tools: &[ToolDefinition],
        on_event: &mut (dyn FnMut(ModelStreamEvent) + Send),
    ) -> Result<ModelResult, Error>;
}
