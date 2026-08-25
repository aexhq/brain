use super::sessions::END_ACCEPTED_STATUS;
use axum::http::StatusCode;
use std::collections::BTreeSet;

fn normalized(path: &str) -> String {
    path.replace("{id}", "{session_id}")
}

#[test]
fn every_public_router_path_is_documented_and_every_documented_path_is_live() {
    let source = include_str!("../api/mod.rs");
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

    assert_eq!(router_paths.len(), 40, "public route inventory changed");
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

/// Every `api_code("...")` literal in the api module must satisfy the open ApiErrorCode
/// pattern, so a typo fails here instead of panicking inside an error path at runtime.
#[test]
fn every_api_code_literal_parses() {
    let sources = [
        include_str!("../api/mod.rs"),
        include_str!("../api/children.rs"),
        include_str!("../api/customer_ws.rs"),
        include_str!("../api/error.rs"),
        include_str!("../api/sandbox_files.rs"),
        include_str!("../api/sessions.rs"),
        include_str!("../api/sse.rs"),
        include_str!("../api/storage.rs"),
    ];
    let mut checked = 0;
    for source in sources {
        for (index, _) in source.match_indices("api_code(\"") {
            let rest = &source[index + "api_code(\"".len()..];
            let literal = &rest[..rest.find('"').expect("terminated literal")];
            literal
                .parse::<brain_protocol::session::ApiErrorCode>()
                .unwrap_or_else(|error| panic!("api_code({literal:?}) is invalid: {error}"));
            checked += 1;
        }
    }
    assert!(
        checked > 20,
        "expected the api error table, found {checked} literals"
    );
}
