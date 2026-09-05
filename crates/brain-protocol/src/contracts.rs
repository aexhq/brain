//! The published contracts, rendered from the Rust types in this crate.
//!
//! `brain-contracts` writes these documents under `contracts/`; CI regenerates them and
//! fails on a diff, so the files there are output. To change a contract, change the
//! type it is rendered from.

use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings};
use serde_json::{Map, Value, json};

use crate::{
    AGENTLOOP_CONTRACT_VERSION, AgentloopAdmission, AgentloopIdentity, ApiError, BoundTool,
    CreateSessionRequest, EnvironmentCallRequest, EnvironmentCallResult, EnvironmentCommand,
    EnvironmentId, EnvironmentResponse, EventPage, HostCommand, HostEvent, HostEventAck, HostId,
    HostRegistration, HostResult, Identity, Message, MessageRequest, Outcome, SESSION_CONTRACT,
    SessionEnvironment, SessionId, SessionList, SessionSummary, ToolAdmission, ToolIdentity,
    ToolManifest, TurnInput, TurnOutput,
};

/// Where the contracts are published; each document's `$id` is its path under here.
pub const CONTRACT_BASE_URL: &str = "https://github.com/aexhq/brain/contracts";

/// The session API: what the HTTP surface accepts and answers.
pub fn session() -> Value {
    document(
        "session/v1/schemas.json",
        "Brain Session API v1",
        |generator| {
            define::<AgentloopAdmission>(generator);
            define::<AgentloopIdentity>(generator);
            define::<ApiError>(generator);
            define::<BoundTool>(generator);
            define::<CreateSessionRequest>(generator);
            define::<SessionEnvironment>(generator);
            define::<EnvironmentCallRequest>(generator);
            define::<EnvironmentCallResult>(generator);
            define::<EnvironmentId>(generator);
            define::<EventPage>(generator);
            define::<HostCommand>(generator);
            define::<HostEvent>(generator);
            define::<HostEventAck>(generator);
            define::<HostId>(generator);
            define::<HostRegistration>(generator);
            define::<HostResult>(generator);
            define::<Identity>(generator);
            define::<Message>(generator);
            define::<MessageRequest>(generator);
            define::<Outcome>(generator);
            define::<ToolAdmission>(generator);
            define::<ToolIdentity>(generator);
            define::<SessionId>(generator);
            define::<SessionList>(generator);
            define::<SessionSummary>(generator);
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

/// The tool manifest: the only thing Brain and environments read about a tool.
pub fn tool() -> Value {
    document("tool/v1/schemas.json", "Brain Tool v1", |generator| {
        root(reference::<ToolManifest>(generator))
    })
}

/// One turn as the agent loop sees it: what Brain hands over and what comes back.
pub fn agentloop() -> Value {
    document(
        "agentloop/v1/contract.json",
        "Brain Agentloop",
        |generator| {
            let input = reference::<TurnInput>(generator);
            let output = reference::<TurnOutput>(generator);
            root(json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["contract", "input", "output"],
                "properties": {
                    "contract": { "const": AGENTLOOP_CONTRACT_VERSION },
                    "input": input,
                    "output": output
                }
            }))
        },
    )
}

/// The closed code sets: journal record kinds, failure codes, API error codes.
pub fn codes() -> Value {
    crate::codes::catalogue()
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
