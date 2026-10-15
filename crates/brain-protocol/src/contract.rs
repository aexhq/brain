//! The published contracts, rendered from the Rust types in this crate.
//!
//! `cargo run -p brain-protocol --bin brain-protocol-contract` writes these documents under this crate's
//! `generated/contract/`; CI regenerates them and fails on a diff, so the files there are
//! output. To change a contract, change the type it is rendered from.

use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings};
use serde_json::{Map, Value, json};

use crate::{
    ApiError, CreateSessionRequest, Environment, EnvironmentCallRequest, EnvironmentCallResult,
    EnvironmentCommand, EnvironmentName, EnvironmentResponse, EventPage, HARNESS_PROTOCOL,
    HostCommand, HostEvent, HostEventAck, HostId, HostRegistration, HostResult, Message,
    MessageRequest, Outcome, SESSION_CONTRACT, SessionId, SessionList, SessionSummary, Tool,
    ToolAdmission, ToolId, TurnEmitRequest, TurnInput, TurnOutput,
};

/// Where the contracts are published; each document's `$id` is its path under here.
pub const CONTRACT_BASE_URL: &str =
    "https://github.com/aexhq/brain/crates/brain-protocol/generated/contract";

/// The session API: what the HTTP surface accepts and answers.
pub fn session() -> Value {
    document(
        "session/v1/schemas.json",
        "Brain Session API v1",
        |generator| {
            define::<crate::BrainPreparation>(generator);
            define::<crate::ProgramAdmission>(generator);
            define::<ApiError>(generator);
            define::<CreateSessionRequest>(generator);
            define::<crate::HarnessCommand>(generator);
            define::<crate::AgentloopRegistration>(generator);
            define::<crate::HarnessInitialize>(generator);
            define::<crate::HarnessIdentity>(generator);
            define::<crate::HarnessSession>(generator);
            define::<crate::HarnessRun>(generator);
            define::<crate::HarnessCallback>(generator);
            define::<Environment>(generator);
            define::<EnvironmentCommand>(generator);
            define::<EnvironmentResponse>(generator);
            define::<crate::EnvironmentControlRequest>(generator);
            define::<crate::EnvironmentView>(generator);
            define::<crate::EnvironmentOutput>(generator);
            define::<crate::EnvironmentEvent>(generator);
            define::<EnvironmentCallRequest>(generator);
            define::<EnvironmentCallResult>(generator);
            define::<EnvironmentName>(generator);
            define::<EventPage>(generator);
            define::<crate::ExecutionCall>(generator);
            define::<crate::ExecutionCallback>(generator);
            define::<HostCommand>(generator);
            define::<crate::HostActivity>(generator);
            define::<crate::HostSuspendRequest>(generator);
            define::<crate::HostSuspendResult>(generator);
            define::<HostEvent>(generator);
            define::<HostEventAck>(generator);
            define::<HostId>(generator);
            define::<HostRegistration>(generator);
            define::<HostResult>(generator);
            define::<crate::HostModelRequest>(generator);
            define::<crate::HostServiceRequest>(generator);
            define::<Message>(generator);
            define::<MessageRequest>(generator);
            define::<crate::ModelList>(generator);
            define::<crate::ModelRequest>(generator);
            define::<crate::ModelResult>(generator);
            define::<crate::ToolResult>(generator);
            define::<crate::ToolReturn>(generator);
            define::<crate::ToolInvocation>(generator);
            define::<TurnInput>(generator);
            define::<Outcome>(generator);
            define::<Tool>(generator);
            define::<ToolAdmission>(generator);
            define::<ToolId>(generator);
            define::<SessionId>(generator);
            define::<SessionList>(generator);
            define::<SessionSummary>(generator);
            define::<crate::SessionTranscript>(generator);
            define::<TurnEmitRequest>(generator);
            define::<crate::KvPutRequest>(generator);
            marker(SESSION_CONTRACT)
        },
    )
}

/// The wire between Brain and an environment: every command and every receipt.
pub fn environment() -> Value {
    document(
        "environment/v1/schemas.json",
        "Brain Environment v1",
        |generator| {
            let command = reference::<EnvironmentCommand>(generator);
            let response = reference::<EnvironmentResponse>(generator);
            root(json!({ "oneOf": [command, response] }))
        },
    )
}

/// One Tool as a session declares it, and as its Environment receives it at invoke.
pub fn tool() -> Value {
    document("tool/v1/schemas.json", "Brain Tool v1", |generator| {
        root(reference::<Tool>(generator))
    })
}

/// Startup lifecycle payloads and one activation's input and output.
pub fn harness() -> Value {
    document("harness/v1/contract.json", "Brain Harness", |generator| {
        define::<crate::AgentloopRegistration>(generator);
        define::<crate::HarnessInitialize>(generator);
        define::<crate::HarnessIdentity>(generator);
        define::<crate::HarnessSession>(generator);
        define::<crate::HarnessRun>(generator);
        define::<crate::HarnessCallback>(generator);
        let input = reference::<TurnInput>(generator);
        let output = reference::<TurnOutput>(generator);
        root(json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["contract", "input", "output"],
            "properties": {
                "contract": { "const": HARNESS_PROTOCOL },
                "input": input,
                "output": output
            }
        }))
    })
}

/// The closed code sets: journal record kinds, failure codes, API error codes.
pub fn codes() -> Value {
    crate::codes::catalogue()
}

/// The Go runtime's public and worker contracts, authored only in Rust.
pub fn session_v2() -> Value {
    use crate::session_v2::*;
    document(
        "session/v2/schemas.json",
        "Brain Session API v2",
        |generator| {
            define::<ErrorResponse>(generator);
            define::<SessionCreate>(generator);
            define::<SessionPage>(generator);
            define::<Input>(generator);
            define::<CommandReceipt>(generator);
            define::<RecordPage>(generator);
            define::<Run>(generator);
            define::<Operation>(generator);
            define::<Claim>(generator);
            define::<Execution>(generator);
            define::<Receipt>(generator);
            define::<UsageSupplement>(generator);
            define::<UsageEvidence>(generator);
            define::<UnavailablePayload>(generator);
            define::<Retry>(generator);
            define::<Interaction>(generator);
            define::<InteractionAnswer>(generator);
            define::<ConnectionAttach>(generator);
            define::<Connection>(generator);
            define::<DocumentUpdate>(generator);
            define::<Document>(generator);
            define::<FileInfo>(generator);
            define::<Schedule>(generator);
            define::<Delegation>(generator);
            define::<Handoff>(generator);
            define::<NodeInfo>(generator);
            define::<TransferCreate>(generator);
            define::<Transfer>(generator);
            define::<ParticipantGrant>(generator);
            define::<ParticipantToken>(generator);
            define::<IdentityIssuer>(generator);
            define::<IdentityView>(generator);
            define::<PackageAdmission>(generator);
            define::<Package>(generator);
            define::<ManagedInvocation>(generator);
            define::<PackageBuildResult>(generator);
            define::<CatalogEntry>(generator);
            define::<ModelRequest>(generator);
            define::<ModelResponse>(generator);
            define::<HarnessRequest>(generator);
            define::<Decision>(generator);
            define::<ChildResults>(generator);
            define::<SessionBudget>(generator);
            define::<StorageReport>(generator);
            define::<ExportManifest>(generator);
            marker("brain-session/2")
        },
    )
}

pub fn integrations() -> Value {
    let entries: Vec<crate::session_v2::CatalogEntry> =
        serde_json::from_str(include_str!("../../../catalog/integrations.json"))
            .expect("integration catalog matches its canonical Rust type");
    serde_json::to_value(entries).expect("integration catalog serializes")
}

/// A contract identifier as a document root, for readers to match on.
fn marker(contract: &str) -> Map<String, Value> {
    root(json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["contract"],
        "properties": { "contract": { "const": contract } }
    }))
}

fn root(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("a contract root is an object, not {other}"),
    }
}

fn define<T: JsonSchema>(generator: &mut SchemaGenerator) {
    generator.subschema_for::<T>();
}

fn reference<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    generator.subschema_for::<T>().to_value()
}

fn document(
    path: &str,
    title: &str,
    build: impl FnOnce(&mut SchemaGenerator) -> Map<String, Value>,
) -> Value {
    let mut generator = SchemaSettings::draft2020_12().into_generator();
    let root = build(&mut generator);
    let definitions = generator.take_definitions(true);
    let mut document = Map::new();
    document.insert(
        "$schema".into(),
        json!("https://json-schema.org/draft/2020-12/schema"),
    );
    document.insert("$id".into(), json!(format!("{CONTRACT_BASE_URL}/{path}")));
    document.insert("title".into(), json!(title));
    document.insert("$defs".into(), Value::Object(definitions));
    document.extend(root);
    let mut document = Value::Object(document);
    absent_not_null(&mut document);
    document
}

/// An `Option` field renders as its type or `null`. On the wire, absence is how Brain
/// says none and Brain never writes `null`, so the contract says absent: the type alone,
/// and the field not required.
fn absent_not_null(value: &mut Value) {
    let null = json!({ "type": "null" });
    match value {
        Value::Object(map) => {
            let kept = match map.get("anyOf") {
                Some(Value::Array(options)) if options.len() == 2 => options
                    .iter()
                    .find(|option| **option != null)
                    .filter(|_| options.contains(&null))
                    .cloned(),
                _ => None,
            };
            if let Some(Value::Object(kept)) = kept {
                map.remove("anyOf");
                for (key, value) in kept {
                    map.entry(key).or_insert(value);
                }
            }
            let single = match map.get("type") {
                Some(Value::Array(types)) if types.len() == 2 && types.contains(&json!("null")) => {
                    types.iter().find(|kind| **kind != json!("null")).cloned()
                }
                _ => None,
            };
            if let Some(single) = single {
                map.insert("type".into(), single);
            }
            map.values_mut().for_each(absent_not_null);
        }
        Value::Array(items) => items.iter_mut().for_each(absent_not_null),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ENVIRONMENT_CONTRACT;

    #[test]
    fn the_environment_contract_marks_every_message() {
        let contract = environment();
        let command = &contract["$defs"]["EnvironmentCommand"]["properties"]["contract"];
        assert_eq!(command["const"], ENVIRONMENT_CONTRACT);
        let response = &contract["$defs"]["EnvironmentResponse"]["properties"]["contract"];
        assert_eq!(response["const"], ENVIRONMENT_CONTRACT);
    }
}
