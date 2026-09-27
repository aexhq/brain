//! The OpenAPI document of the session API, rendered from the route annotations in
//! `router.rs` and the schemas in `brain-protocol`.
//!
//! `cargo run -p brain-http --bin brain-http-contract` writes [`openapi`] to this crate's
//! `generated/contract/session/v1/openapi.yaml`. The
//! router is built from the same annotations, and [`crate::router`] refuses to start if
//! the two disagree, so a route cannot exist without appearing in the document.

use std::collections::BTreeSet;

use serde_json::Value;
use utoipa::{
    OpenApi as _, PartialSchema, ToSchema,
    openapi::{
        KnownFormat, ObjectBuilder, RefOr, Schema, SchemaFormat,
        schema::{SchemaType, Type},
    },
};

use crate::router::ApiDoc;

pub(crate) struct Security;

impl utoipa::Modify for Security {
    fn modify(&self, document: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};

        let components = document.components.get_or_insert_with(Default::default);
        for (name, description) in [
            (
                "serverToken",
                "The server's BRAIN_API_TOKEN, sent as Authorization: Bearer <token>. Required for application API calls when configured; optional only on a loopback server started without a token.",
            ),
            (
                "hostToken",
                "The scoped token returned by POST /v1/hosts, sent as Authorization: Bearer <token>. Identifies this registered host; do not substitute the server API token.",
            ),
            (
                "invocationToken",
                "The invocation-scoped services token Brain supplies to an Environment executing an agent loop, tool or controller. Send it as Authorization: Bearer <token>; valid only for the granted services while that execution is open.",
            ),
            (
                "reporterToken",
                "The reporter token Brain supplies to an Environment instance, sent as Authorization: Bearer <token>. Scoped to the session, environment name and incarnation sequence in the URL.",
            ),
        ] {
            components.add_security_scheme(
                name,
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .description(Some(description))
                        .build(),
                ),
            );
        }
    }
}

/// Protocol types as the OpenAPI document names them: references into the session
/// contract's definitions, which [`openapi`] embeds as the document's components. The
/// schemas themselves are never restated here; `brain-protocol` renders them once.
pub(crate) mod contract {
    use std::borrow::Cow;

    use schemars::JsonSchema as _;
    use utoipa::{
        PartialSchema, ToSchema,
        openapi::{Ref, RefOr, Schema},
    };

    macro_rules! referenced {
        ($($name:ident),* $(,)?) => {$(
            pub(crate) struct $name;

            impl PartialSchema for $name {
                fn schema() -> RefOr<Schema> {
                    RefOr::Ref(Ref::from_schema_name(brain_protocol::$name::schema_name()))
                }
            }

            impl ToSchema for $name {
                fn name() -> Cow<'static, str> {
                    brain_protocol::$name::schema_name()
                }
            }
        )*};
    }

    referenced!(
        AgentloopAdmission,
        AgentloopId,
        ApiError,
        CreateSessionRequest,
        EnvironmentCallRequest,
        EnvironmentCallResult,
        EnvironmentName,
        EnvironmentEvent,
        EnvironmentControlRequest,
        EventPage,
        HostId,
        HostEvent,
        HostEventAck,
        HostRegistration,
        HostResult,
        HostModelRequest,
        HostServiceRequest,
        ModelResult,
        MessageRequest,
        ModelList,
        SessionId,
        SessionList,
        SessionSummary,
        SessionTranscript,
        ToolAdmission,
        ExecutionCall,
    );
}

/// The bytes of an agentloop package, as `POST /v1/agentloops` receives them.
pub(crate) struct Package;

impl PartialSchema for Package {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(SchemaType::Type(Type::String))
            .format(Some(SchemaFormat::KnownFormat(KnownFormat::Binary)))
            .into()
    }
}

impl ToSchema for Package {}

/// The session API's OpenAPI document, self-contained: every schema the operations
/// reference is embedded under `components.schemas` from the session contract.
pub fn openapi() -> Value {
    let mut document =
        serde_json::to_value(ApiDoc::openapi()).expect("the OpenAPI document serializes");
    let mut schemas = brain_protocol::contract::session()["$defs"].take();
    rewrite_references(&mut schemas);
    document["components"]["schemas"] = schemas;
    document
}

/// The session contract addresses its definitions as `#/$defs/Name`; inside the
/// OpenAPI document they live at `#/components/schemas/Name`.
fn rewrite_references(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if key == "$ref"
                    && let Some(target) = child.as_str().and_then(|s| s.strip_prefix("#/$defs/"))
                {
                    *child = Value::String(format!("#/components/schemas/{target}"));
                } else {
                    rewrite_references(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(rewrite_references),
        _ => {}
    }
}

/// Every operation a document declares, as `METHOD path`.
pub(crate) fn operations(document: &utoipa::openapi::OpenApi) -> BTreeSet<String> {
    const METHODS: [&str; 8] = [
        "get", "put", "post", "delete", "options", "head", "patch", "trace",
    ];
    let paths = serde_json::to_value(&document.paths).expect("paths serialize");
    let mut operations = BTreeSet::new();
    for (path, item) in paths.as_object().into_iter().flatten() {
        for method in METHODS {
            if item.get(method).is_some() {
                operations.insert(format!("{} {path}", method.to_uppercase()));
            }
        }
    }
    operations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn references(value: &Value, found: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if key == "$ref" {
                        found.push(child.as_str().unwrap_or_default().to_owned());
                    } else {
                        references(child, found);
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| references(item, found)),
            _ => {}
        }
    }

    #[test]
    fn every_reference_in_the_document_resolves() {
        let document = openapi();
        let schemas = document["components"]["schemas"]
            .as_object()
            .expect("components are embedded");
        let mut found = Vec::new();
        references(&document, &mut found);
        assert!(!found.is_empty());
        for reference in found {
            let name = reference
                .strip_prefix("#/components/schemas/")
                .unwrap_or_else(|| panic!("{reference} is not a component reference"));
            assert!(schemas.contains_key(name), "{reference} names no component");
        }
    }

    #[test]
    fn authentication_matches_the_route_callers_and_control_documents_its_key() {
        let document = openapi();
        for (path, method, scheme) in [
            ("/v1/sessions", "post", "serverToken"),
            ("/v1/hosts", "post", "serverToken"),
            ("/v1/hosts/{host_id}/commands", "get", "hostToken"),
            (
                "/v1/sessions/{session_id}/executions/{sequence}/call",
                "post",
                "invocationToken",
            ),
            (
                "/v1/sessions/{session_id}/environments/{environment}/{sequence}/events",
                "post",
                "reporterToken",
            ),
        ] {
            assert_eq!(
                document["paths"][path][method]["security"],
                serde_json::json!([{scheme: []}])
            );
            assert_eq!(
                document["components"]["securitySchemes"][scheme]["scheme"],
                "bearer"
            );
        }
        for path in ["/health/live", "/health/ready"] {
            assert!(document["paths"][path]["get"].get("security").is_none());
        }
        let parameters =
            document["paths"]["/v1/sessions/{session_id}/environments"]["post"]["parameters"]
                .as_array()
                .unwrap();
        let key = parameters
            .iter()
            .find(|parameter| parameter["name"] == "Idempotency-Key")
            .unwrap();
        assert_eq!(key["in"], "header");
        assert_eq!(key["required"], true);
    }
}
