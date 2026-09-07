use async_trait::async_trait;
use brain_protocol::{
    ModelBinding, ModelRequest, ModelResult, ModelStreamEvent, SessionId, ToolDefinition,
};

mod accumulator;
mod anthropic;
#[cfg(test)]
mod continuation_tests;
mod generated;
mod http;
mod openai;
mod registry;
mod responses;
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

fn validate_image(url: &str) -> Result<(), Error> {
    if let Some(data) = url.strip_prefix("data:image/") {
        use base64::Engine as _;
        let (kind, encoded) = data
            .split_once(";base64,")
            .ok_or_else(|| Error::InvalidState("image data URL must contain base64".into()))?;
        if !matches!(kind, "png" | "jpeg" | "gif" | "webp")
            || base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .is_err()
        {
            return Err(Error::InvalidState("invalid image data URL".into()));
        }
    } else {
        let parsed =
            reqwest::Url::parse(url).map_err(|error| Error::InvalidState(error.to_string()))?;
        if parsed.scheme() != "https"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(Error::InvalidState(
                "image URL must use HTTPS without credentials".into(),
            ));
        }
    }
    Ok(())
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
