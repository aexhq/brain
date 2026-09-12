#[cfg(test)]
mod tests {
    use crate::model::{CATALOG, Dialect};
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
    fn the_direct_providers_are_in_the_catalog_with_their_dialects() {
        let find = |name: &str| CATALOG.iter().find(|row| row.name == name).unwrap();
        assert_eq!(find("openai").dialect, Dialect::OpenAiResponses);
        assert_eq!(find("anthropic").dialect, Dialect::AnthropicMessages);
    }
}
