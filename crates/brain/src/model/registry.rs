//! The provider catalog: which providers Brain can dispatch to, and what each
//! one speaks. `ModelSelection.provider` is resolved here, once, instead of
//! being string-compared at every layer.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    OpenAiChat,
    AnthropicMessages,
}

#[derive(Clone, Copy, Debug)]
pub struct ProviderSpec {
    pub name: &'static str,
    pub dialect: Dialect,
    pub default_base_url: &'static str,
    /// The gateway namespaces model ids as `provider/model`; direct providers
    /// use bare ids.
    pub namespaced_model: bool,
    /// Whether the wire accepts a `response_format` parameter. A dialect that
    /// does not gets a typed rejection instead of a silently dropped field.
    pub supports_response_format: bool,
}

pub const PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        name: "vercel-ai-gateway",
        dialect: Dialect::OpenAiChat,
        default_base_url: "https://ai-gateway.vercel.sh/v1",
        namespaced_model: true,
        supports_response_format: true,
    },
    ProviderSpec {
        name: "openai",
        dialect: Dialect::OpenAiChat,
        default_base_url: "https://api.openai.com/v1",
        namespaced_model: false,
        supports_response_format: true,
    },
    ProviderSpec {
        name: "anthropic",
        dialect: Dialect::AnthropicMessages,
        default_base_url: "https://api.anthropic.com/v1",
        namespaced_model: false,
        supports_response_format: false,
    },
];

pub fn provider_spec(name: &str) -> Option<&'static ProviderSpec> {
    PROVIDERS.iter().find(|spec| spec.name == name)
}

/// Model-name shape for one provider. The gateway requires a `provider/model`
/// namespace; direct providers take a bare id. Neither takes whitespace.
pub fn valid_model_name(spec: &ProviderSpec, name: &str) -> bool {
    if name.is_empty() || name.len() > 256 || name.chars().any(char::is_whitespace) {
        return false;
    }
    if spec.namespaced_model {
        name.split_once('/')
            .is_some_and(|(namespace, model)| !namespace.is_empty() && !model.is_empty())
    } else {
        !name.starts_with('/')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_resolves_and_unknown_ones_do_not() {
        for spec in PROVIDERS {
            assert_eq!(provider_spec(spec.name).unwrap().name, spec.name);
        }
        assert!(provider_spec("bedrock").is_none());
    }

    #[test]
    fn model_names_are_validated_per_provider() {
        let gateway = provider_spec("vercel-ai-gateway").unwrap();
        assert!(valid_model_name(gateway, "openai/gpt-5-mini"));
        assert!(!valid_model_name(gateway, "gpt-5-mini"));
        assert!(!valid_model_name(gateway, "openai/"));
        let anthropic = provider_spec("anthropic").unwrap();
        assert!(valid_model_name(anthropic, "claude-sonnet-4-5"));
        assert!(!valid_model_name(anthropic, "claude sonnet"));
        assert!(!valid_model_name(anthropic, ""));
    }
}
