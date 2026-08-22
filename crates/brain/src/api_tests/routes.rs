use super::END_ACCEPTED_STATUS;
use axum::http::StatusCode;
use std::collections::BTreeSet;

fn normalized(path: &str) -> String {
    path.replace("{id}", "{session_id}")
}

#[test]
fn every_public_router_path_is_documented_and_every_documented_path_is_live() {
    let source = include_str!("../api.rs");
    let route = regex::Regex::new(r#"(?s)\.route\(\s*\"([^\"]+)\""#).unwrap();
    let router_paths = route
        .captures_iter(source)
        .map(|capture| capture[1].to_owned())
        .filter(|path| path.starts_with("/v1/") && !path.starts_with("/internal/"))
        .map(|path| normalized(&path))
        .collect::<BTreeSet<_>>();

    let contract = include_str!("../../../../contracts/session/v1/openapi.yaml");
    let mut in_paths = false;
    let mut openapi_paths = BTreeSet::new();
    for line in contract.lines() {
        if line == "paths:" {
            in_paths = true;
            continue;
        }
        if line == "components:" {
            break;
        }
        if in_paths
            && let Some(path) = line
                .strip_prefix("  /")
                .and_then(|line| line.strip_suffix(':'))
        {
            openapi_paths.insert(format!("/{path}"));
        }
    }

    assert_eq!(router_paths.len(), 37, "public route inventory changed");
    assert_eq!(router_paths, openapi_paths);
}

#[test]
fn root_and_child_end_share_the_async_acceptance_status() {
    assert_eq!(END_ACCEPTED_STATUS, StatusCode::ACCEPTED);
    let contract = include_str!("../../../../contracts/session/v1/openapi.yaml");
    for path in [
        "/v1/sessions/{session_id}/end",
        "/v1/sessions/{session_id}/children/{child_id}/end",
    ] {
        let marker = format!("  {path}:");
        let tail = contract
            .split_once(&marker)
            .unwrap_or_else(|| panic!("missing OpenAPI path {path}"))
            .1;
        let operation = tail.split("\n  /").next().expect("path operation section");
        assert!(operation.contains("\n    post:"));
        assert!(operation.contains("\n        \"202\":"));
        assert!(!operation.contains("\n        \"200\":"));
    }
}
