//! The provider registry: which providers Brain can dispatch to, and what each
//! one speaks. `ModelSelection.provider` is resolved here, once, instead of
//! being string-compared at every layer.
//!
//! A [`ProviderDef`] is the normalized layer every source feeds: the generated
//! models.dev catalog (`generated::CATALOG`), the curated built-ins, and an
//! operator's providers file all describe providers in this one shape. The set
//! of valid providers is a property of the deployment, not the protocol, which
//! is why the session contract accepts any identifier-shaped provider name and
//! admission happens here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::KernelError;

use super::generated::{CATALOG, CatalogModel, CatalogProvider};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Dialect {
    #[serde(rename = "openai_chat")]
    OpenAiChat,
    #[serde(rename = "anthropic_messages")]
    AnthropicMessages,
}

/// Which wire field carries the output-token cap in the OpenAI dialect.
/// OpenAI itself deprecated `max_tokens` for `max_completion_tokens`; most
/// OpenAI-compatible servers only know the original field.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MaxTokensField {
    #[default]
    MaxCompletionTokens,
    MaxTokens,
}

/// The normalized provider definition. Owned and built once at startup; the
/// serde shape doubles as the operator's providers-file format.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDef {
    pub name: String,
    pub dialect: Dialect,
    pub base_url: String,
    /// The gateway namespaces model ids as `provider/model`; direct providers
    /// use bare ids.
    #[serde(default)]
    pub namespaced_model: bool,
    /// Whether the wire accepts a `response_format` parameter. A dialect that
    /// does not gets a typed rejection instead of a silently dropped field.
    #[serde(default = "default_true")]
    pub supports_response_format: bool,
    #[serde(default)]
    pub max_tokens_field: MaxTokensField,
    /// Advisory metadata for models this provider is known to serve. Admission
    /// is open: an id not listed here still passes syntactic validation.
    #[serde(default)]
    pub models: Vec<ModelDef>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDef {
    pub id: String,
    #[serde(default)]
    pub context_window_tokens: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub structured_output: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub cost: Option<ModelCost>,
}

/// USD per million tokens.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCost {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

fn default_true() -> bool {
    true
}

/// The composed provider set a deployment dispatches against. Sources merge in
/// a fixed order, later superseding earlier by name: the generated catalog,
/// then the curated built-ins, then the operator's custom providers, then the
/// legacy base-URL override flags as URL-only patches.
pub struct ProviderRegistry {
    providers: BTreeMap<String, ProviderDef>,
}

impl ProviderRegistry {
    pub fn compose(
        custom: Vec<ProviderDef>,
        base_url_overrides: &[(String, String)],
    ) -> Result<Self, KernelError> {
        let mut providers = BTreeMap::new();
        for row in CATALOG {
            let def = ProviderDef::from(row);
            providers.insert(def.name.clone(), def);
        }
        for def in curated() {
            providers.insert(def.name.clone(), def);
        }
        for def in custom {
            if !valid_provider_name(&def.name) {
                return Err(KernelError::InvalidState(format!(
                    "custom provider name {:?} is not a valid identifier",
                    def.name
                )));
            }
            super::validate_base_url(&def.base_url)?;
            if providers.insert(def.name.clone(), def.clone()).is_some() {
                // Overriding a catalog provider is how an operator fixes a bad
                // upstream row without waiting for a release; it means the
                // deployment diverges from the shipped catalog, so say so.
                tracing::info!(provider = %def.name, "custom providers file overrides a built-in provider");
            }
        }
        for (name, url) in base_url_overrides {
            // A typo here would silently leave the default endpoint in place.
            let Some(def) = providers.get_mut(name) else {
                return Err(KernelError::InvalidState(format!(
                    "unknown model provider {name} in base URL overrides"
                )));
            };
            super::validate_base_url(url)?;
            def.base_url = url.clone();
        }
        Ok(Self { providers })
    }

    /// The registry a caller gets with no configuration at all: catalog plus
    /// curated built-ins.
    pub fn default_set() -> Self {
        Self::compose(Vec::new(), &[]).expect("the built-in provider set composes")
    }

    pub fn get(&self, name: &str) -> Option<&ProviderDef> {
        self.providers.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ProviderDef> {
        self.providers.values()
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn model(&self, provider: &str, id: &str) -> Option<&ModelDef> {
        self.get(provider)?
            .models
            .iter()
            .find(|model| model.id == id)
    }

    /// Whether `response_format` may be sent to this provider and model. False
    /// when the provider cannot carry it, or when the model is known and known
    /// not to support structured output. An unknown model on a capable
    /// provider passes: admission is open and the metadata is advisory.
    pub fn supports_response_format(&self, provider: &str, model: &str) -> bool {
        let Some(def) = self.get(provider) else {
            return false;
        };
        if !def.supports_response_format {
            return false;
        }
        self.model(provider, model)
            .and_then(|known| known.structured_output)
            .unwrap_or(true)
    }
}

/// The hand-maintained entries: the gateway (not in the catalog — models.dev
/// files it under a dedicated SDK) and pins for `openai`/`anthropic`, so a
/// snapshot refresh can never mutate the semantics deploys already rely on.
fn curated() -> Vec<ProviderDef> {
    let plain = |name: &str, dialect: Dialect, base_url: &str, response_format: bool| ProviderDef {
        name: name.into(),
        dialect,
        base_url: base_url.into(),
        namespaced_model: false,
        supports_response_format: response_format,
        max_tokens_field: MaxTokensField::MaxCompletionTokens,
        models: CATALOG
            .iter()
            .find(|row| row.name == name)
            .map(|row| row.models.iter().map(ModelDef::from).collect())
            .unwrap_or_default(),
    };
    vec![
        ProviderDef {
            name: "vercel-ai-gateway".into(),
            dialect: Dialect::OpenAiChat,
            base_url: "https://ai-gateway.vercel.sh/v1".into(),
            namespaced_model: true,
            supports_response_format: true,
            max_tokens_field: MaxTokensField::MaxCompletionTokens,
            models: Vec::new(),
        },
        plain(
            "openai",
            Dialect::OpenAiChat,
            "https://api.openai.com/v1",
            true,
        ),
        plain(
            "anthropic",
            Dialect::AnthropicMessages,
            "https://api.anthropic.com/v1",
            false,
        ),
    ]
}

impl From<&CatalogProvider> for ProviderDef {
    fn from(row: &CatalogProvider) -> Self {
        Self {
            name: row.name.into(),
            dialect: row.dialect,
            base_url: row.base_url.into(),
            namespaced_model: false,
            supports_response_format: row.supports_response_format,
            max_tokens_field: row.max_tokens_field,
            models: row.models.iter().map(ModelDef::from).collect(),
        }
    }
}

impl From<&CatalogModel> for ModelDef {
    fn from(row: &CatalogModel) -> Self {
        Self {
            id: row.id.into(),
            context_window_tokens: row.context_window_tokens,
            max_output_tokens: row.max_output_tokens,
            tool_call: row.tool_call,
            structured_output: row.structured_output,
            reasoning: row.reasoning,
            cost: row
                .cost
                .map(|(input, output, cache_read, cache_write)| ModelCost {
                    input,
                    output,
                    cache_read,
                    cache_write,
                }),
        }
    }
}

/// The session contract's `Identifier` shape, which provider names must fit.
pub fn valid_provider_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b':' | b'-'))
        })
}

/// Model-name shape for one provider. The gateway requires a `provider/model`
/// namespace; direct providers take a bare id. Neither takes whitespace.
pub fn valid_model_name(def: &ProviderDef, name: &str) -> bool {
    if name.is_empty() || name.len() > 256 || name.chars().any(char::is_whitespace) {
        return false;
    }
    if def.namespaced_model {
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
    fn every_curated_provider_resolves_and_unknown_ones_do_not() {
        let registry = ProviderRegistry::default_set();
        for name in ["vercel-ai-gateway", "openai", "anthropic"] {
            assert_eq!(registry.get(name).unwrap().name, name);
        }
        assert!(registry.get("bedrock").is_none());
    }

    #[test]
    fn model_names_are_validated_per_provider() {
        let registry = ProviderRegistry::default_set();
        let gateway = registry.get("vercel-ai-gateway").unwrap();
        assert!(valid_model_name(gateway, "openai/gpt-5-mini"));
        assert!(!valid_model_name(gateway, "gpt-5-mini"));
        assert!(!valid_model_name(gateway, "openai/"));
        let anthropic = registry.get("anthropic").unwrap();
        assert!(valid_model_name(anthropic, "claude-sonnet-4-5"));
        assert!(!valid_model_name(anthropic, "claude sonnet"));
        assert!(!valid_model_name(anthropic, ""));
    }

    fn custom(name: &str, base_url: &str) -> ProviderDef {
        ProviderDef {
            name: name.into(),
            dialect: Dialect::OpenAiChat,
            base_url: base_url.into(),
            namespaced_model: false,
            supports_response_format: true,
            max_tokens_field: MaxTokensField::MaxTokens,
            models: Vec::new(),
        }
    }

    #[test]
    fn a_custom_provider_is_admitted_and_can_override_a_built_in() {
        let registry = ProviderRegistry::compose(
            vec![
                custom("ollama-local", "http://127.0.0.1:11434/v1"),
                custom("anthropic", "https://proxy.example.com/v1"),
            ],
            &[],
        )
        .unwrap();
        assert_eq!(
            registry.get("ollama-local").unwrap().max_tokens_field,
            MaxTokensField::MaxTokens
        );
        assert_eq!(
            registry.get("anthropic").unwrap().base_url,
            "https://proxy.example.com/v1",
            "the custom file supersedes the built-in definition"
        );
    }

    #[test]
    fn custom_providers_are_validated_at_compose_time() {
        let bad_name =
            ProviderRegistry::compose(vec![custom("not valid!", "https://x.example")], &[]);
        assert!(matches!(bad_name, Err(KernelError::InvalidState(_))));
        let bad_url =
            ProviderRegistry::compose(vec![custom("fine", "https://user:pw@example.com/v1")], &[]);
        assert!(matches!(bad_url, Err(KernelError::InvalidState(_))));
    }

    #[test]
    fn a_typoed_base_url_override_is_rejected_instead_of_silently_ignored() {
        let result = ProviderRegistry::compose(
            Vec::new(),
            &[("open-ai".into(), "https://example.invalid".into())],
        );
        assert!(matches!(result, Err(KernelError::InvalidState(_))));
        let patched = ProviderRegistry::compose(
            Vec::new(),
            &[("anthropic".into(), "https://alt.example.com/v1".into())],
        )
        .unwrap();
        assert_eq!(
            patched.get("anthropic").unwrap().base_url,
            "https://alt.example.com/v1"
        );
    }

    #[test]
    fn response_format_support_resolves_per_provider_and_per_model() {
        let mut def = custom("structured", "https://api.example.com/v1");
        def.models = vec![
            ModelDef {
                id: "yes-model".into(),
                context_window_tokens: None,
                max_output_tokens: None,
                tool_call: None,
                structured_output: Some(true),
                reasoning: None,
                cost: None,
            },
            ModelDef {
                id: "no-model".into(),
                context_window_tokens: None,
                max_output_tokens: None,
                tool_call: None,
                structured_output: Some(false),
                reasoning: None,
                cost: None,
            },
        ];
        let registry = ProviderRegistry::compose(vec![def], &[]).unwrap();
        assert!(registry.supports_response_format("structured", "yes-model"));
        assert!(
            registry.supports_response_format("structured", "unknown-model"),
            "open admission: an unknown model on a capable provider passes"
        );
        assert!(
            !registry.supports_response_format("structured", "no-model"),
            "a model known not to support structured output is gated"
        );
        assert!(!registry.supports_response_format("anthropic", "claude-sonnet-4-5"));
        assert!(!registry.supports_response_format("nonexistent", "anything"));
    }

    #[test]
    fn the_catalog_is_present_and_the_curated_pins_win() {
        let registry = ProviderRegistry::default_set();
        assert!(
            registry.len() > 100,
            "the generated models.dev catalog should register on the order of 150 providers, got {}",
            registry.len()
        );
        let openai = registry.get("openai").unwrap();
        assert_eq!(openai.base_url, "https://api.openai.com/v1");
        assert_eq!(openai.max_tokens_field, MaxTokensField::MaxCompletionTokens);
        assert_eq!(openai.dialect, Dialect::OpenAiChat);
        assert!(
            !openai.models.is_empty(),
            "the curated pin should still carry the catalog's model metadata"
        );
    }
}
