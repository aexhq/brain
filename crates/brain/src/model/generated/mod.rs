//! The models.dev-derived provider catalog. The table itself lives in
//! `catalog.rs`, generated from the pinned snapshot at
//! `catalog/models-dev/api.json`; these are the row types it instantiates.
//! Rows are `&'static` so a malformed refresh is a compile error, not a
//! startup one; `ProviderRegistry` converts them to owned definitions once.

use crate::model::{Dialect, MaxTokensField};

mod catalog;

pub use catalog::{CATALOG, SNAPSHOT_DIGEST};

pub struct CatalogProvider {
    pub name: &'static str,
    pub dialect: Dialect,
    pub base_url: &'static str,
    pub supports_response_format: bool,
    pub max_tokens_field: MaxTokensField,
    pub models: &'static [CatalogModel],
}

pub struct CatalogModel {
    pub id: &'static str,
    pub context_window_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub tool_call: Option<bool>,
    pub structured_output: Option<bool>,
    pub reasoning: Option<bool>,
    /// (input, output, cache_read, cache_write), USD per million tokens.
    pub cost: Option<(f64, f64, Option<f64>, Option<f64>)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{valid_provider_name, validate_base_url};

    #[test]
    fn catalog_names_are_unique_identifiers() {
        let mut seen = std::collections::HashSet::new();
        for row in CATALOG {
            assert!(
                valid_provider_name(row.name),
                "catalog provider {:?} is not identifier-shaped",
                row.name
            );
            assert!(
                seen.insert(row.name),
                "catalog provider {:?} appears twice",
                row.name
            );
        }
        assert!(!CATALOG.is_empty());
    }

    #[test]
    fn every_catalog_base_url_passes_the_transport_rules() {
        for row in CATALOG {
            assert!(
                validate_base_url(row.base_url).is_ok(),
                "catalog provider {:?} carries base URL {:?} the transport would refuse",
                row.name,
                row.base_url
            );
        }
    }

    #[test]
    fn model_ids_are_unique_per_provider() {
        for row in CATALOG {
            let mut seen = std::collections::HashSet::new();
            for model in row.models {
                assert!(
                    seen.insert(model.id),
                    "provider {:?} lists model {:?} twice",
                    row.name,
                    model.id
                );
            }
        }
    }

    #[test]
    fn the_legacy_providers_are_in_the_catalog_with_their_dialects() {
        let find = |name: &str| CATALOG.iter().find(|row| row.name == name).unwrap();
        assert_eq!(find("openai").dialect, Dialect::OpenAiChat);
        assert_eq!(find("anthropic").dialect, Dialect::AnthropicMessages);
    }
}
