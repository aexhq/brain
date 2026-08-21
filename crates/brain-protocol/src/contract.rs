//! Canonical identities for the single current Brain-to-Hand contract.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::hand::{
    Digest, OperationEnvelope, SandboxCopyRequest, SandboxExecutionRequest,
    SandboxFileWriteRequest, TerminalResult, WriteStdinRequest,
};
use crate::session::{ExternalToolCallRequest, ExternalToolCallResponse, ExternalToolDisposition};

/// Canonical JSON Schema for the only supported Brain-to-Hand contract.
pub const HAND_CONTRACT_SCHEMA_JSON: &str = include_str!("../../../contracts/hand/contract.json");

/// Pinned SHA-256 of the schema's RFC 8785 canonical JSON representation.
pub const HAND_CONTRACT_DIGEST: &str = include_str!("../../../contracts/hand/contract.digest");

/// Hash any serializable value using RFC 8785 canonical JSON.
pub fn canonical_digest<T: Serialize>(value: &T) -> Result<Digest, serde_json::Error> {
    let canonical = serde_jcs::to_vec(value)?;
    Ok(hex::encode(Sha256::digest(canonical))
        .parse()
        .expect("SHA-256 hex satisfies the contract Digest schema"))
}

/// Compute the compatibility identity of the embedded contract schema.
pub fn hand_contract_digest() -> Digest {
    let schema: Value =
        serde_json::from_str(HAND_CONTRACT_SCHEMA_JSON).expect("embedded Hand schema is valid");
    canonical_digest(&schema).expect("the embedded Hand schema is canonicalizable")
}

/// Compute an operation request digest over every effect-affecting envelope field.
///
/// `request_digest` is blanked to avoid recursion. Trace metadata is observability-only and is
/// excluded so a transport retry can add a span without changing operation identity.
pub fn operation_request_digest(envelope: &OperationEnvelope) -> Digest {
    let mut value = serde_json::to_value(envelope).expect("an operation envelope serializes");
    let object = value
        .as_object_mut()
        .expect("an operation envelope is a JSON object");
    object.remove("request_digest");
    object.remove("trace");
    canonical_digest(&value).expect("an operation envelope is canonicalizable")
}

/// Compute an additional-sandbox execution identity over every effect-affecting request field.
///
/// The canonical projection is the exact serialized request with only the self-referential
/// `request_digest` member omitted. No target, generation, resource or network field is excluded.
pub fn sandbox_execution_request_digest(request: &SandboxExecutionRequest) -> Digest {
    request_digest_without_self(request)
}

/// Compute a stdin append identity over every effect-affecting request field.
///
/// The canonical projection is the exact serialized request with only the self-referential
/// `request_digest` member omitted. In particular, `operation_id`, target, generation,
/// `execution_id`, the exact text bytes, and the EOF bit all participate. Empty text with EOF
/// false is therefore a distinct, idempotent poll request.
pub fn write_stdin_request_digest(request: &WriteStdinRequest) -> Digest {
    request_digest_without_self(request)
}

/// Compute an inline/object-backed sandbox file write identity over every effect-affecting field.
pub fn sandbox_file_write_request_digest(request: &SandboxFileWriteRequest) -> Digest {
    let mut value = request_value_without_self(request);
    if let Some(authority) = value
        .get_mut("source")
        .and_then(Value::as_object_mut)
        .and_then(|source| source.get_mut("fetch"))
    {
        // A fetch authority names an already sealed immutable ObjectReference. Its reservation id
        // and transport credentials may both be refreshed after a lost response.
        remove_ephemeral_authority(authority, true);
    }
    canonical_digest(&value).expect("a sandbox file write request is canonicalizable")
}

/// Compute a sandbox import/export identity over its exact target, object and stable transfer
/// identity. Short-lived URL/header/expiry capability material may be refreshed on exact retry.
pub fn sandbox_copy_request_digest(request: &SandboxCopyRequest) -> Digest {
    let mut value = request_value_without_self(request);
    if let Some(authority) = value.get_mut("transfer") {
        // Imports fetch the separately sealed `object`, so their short-lived download reservation
        // id is not effect identity. Exports publish into the pending object named by transfer_id;
        // changing that id would change the destination and must conflict.
        remove_ephemeral_authority(
            authority,
            request.direction == crate::hand::SandboxCopyRequestDirection::Import,
        );
    }
    canonical_digest(&value).expect("a sandbox copy request is canonicalizable")
}

fn request_digest_without_self<T: Serialize>(request: &T) -> Digest {
    canonical_digest(&request_value_without_self(request)).expect("a request is canonicalizable")
}

fn request_value_without_self<T: Serialize>(request: &T) -> Value {
    let mut value = serde_json::to_value(request).expect("a request serializes");
    value
        .as_object_mut()
        .expect("a request is a JSON object")
        .remove("request_digest");
    value
}

fn remove_ephemeral_authority(value: &mut Value, remove_transfer_id: bool) {
    if let Some(authority) = value.as_object_mut() {
        // A scoped URL may be refreshed after a lost response without widening the immutable
        // transfer identity. Object identity, method and max_bytes always remain sealed; callers
        // decide whether a source-only transfer_id is also refreshable.
        authority.remove("url");
        authority.remove("headers");
        authority.remove("expires_at_ms");
        if remove_transfer_id {
            authority.remove("transfer_id");
        }
    }
}

/// Compute the terminal acknowledgement identity over the terminal observation.
pub fn terminal_result_digest(terminal: &TerminalResult) -> Digest {
    let mut value = serde_json::to_value(terminal).expect("a terminal result serializes");
    value
        .as_object_mut()
        .expect("a terminal result is a JSON object")
        .remove("terminal_digest");
    canonical_digest(&value).expect("a terminal result is canonicalizable")
}

/// Exact bounded representation enforced by both Brain and Hand before retaining a terminal
/// receipt. Large data belongs in an ObjectReference/session-storage key or sandbox path.
pub fn terminal_inline_bytes(value: &Value) -> Result<usize, serde_json::Error> {
    Ok(serde_jcs::to_vec(value)?.len())
}

pub fn terminal_inline_fits(value: &Value) -> bool {
    terminal_inline_bytes(value).is_ok_and(|bytes| bytes <= crate::MAX_TOOL_TERMINAL_INLINE_BYTES)
}

/// Exact serialized wire size of a trusted host-executor request.
pub fn external_tool_request_wire_bytes(
    request: &ExternalToolCallRequest,
) -> Result<usize, serde_json::Error> {
    Ok(serde_json::to_vec(request)?.len())
}

/// Whether the full request fits the one neutral Brain/Aex executor-ingress ceiling.
pub fn external_tool_request_wire_fits(request: &ExternalToolCallRequest) -> bool {
    external_tool_request_wire_bytes(request)
        .is_ok_and(|bytes| bytes <= crate::MAX_EXTERNAL_TOOL_REQUEST_BYTES)
}

/// Apply Brain's exact inline terminal projections to a host-executor response. Content and the
/// structured result are independently bounded; a terminal disposition additionally bounds the
/// canonical `{value,metadata}` or `{error}` projection that becomes the turn terminal.
pub fn external_tool_response_inline_fits(response: &ExternalToolCallResponse) -> bool {
    if response.content.len() > crate::MAX_TOOL_TERMINAL_INLINE_BYTES
        || response
            .result
            .as_ref()
            .is_some_and(|result| !terminal_inline_fits(result))
    {
        return false;
    }
    let terminal = match response.disposition {
        ExternalToolDisposition::Continue => return true,
        ExternalToolDisposition::CompleteTurn => serde_json::json!({
            "value": response.result,
            "metadata": response.result_metadata,
        }),
        ExternalToolDisposition::FailTurn => serde_json::json!({
            "error": response.error,
        }),
    };
    terminal_inline_fits(&terminal)
}

/// Whether the exact encoded host-executor response fits the neutral transport envelope.
/// Callers should serialize once, enforce this byte bound, and reuse those exact bytes across an
/// ambiguous retry. Semantic terminal limits remain independently enforced by
/// [`external_tool_response_inline_fits`].
pub fn external_tool_response_wire_fits(response: &ExternalToolCallResponse) -> bool {
    serde_json::to_vec(response)
        .is_ok_and(|bytes| bytes.len() <= crate::MAX_EXTERNAL_TOOL_RESPONSE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn external_response(content: String, result: Value) -> ExternalToolCallResponse {
        serde_json::from_value(serde_json::json!({
            "outcome": "completed",
            "content": content,
            "is_error": false,
            "disposition": "continue",
            "result": result,
            "result_metadata": {},
        }))
        .unwrap()
    }

    fn external_request() -> ExternalToolCallRequest {
        serde_json::from_value(serde_json::json!({
            "session_id": "ses_01HZX8Y2K3M4N5P6Q7R8S9T0",
            "turn_id": "trn_01HZX8Y2K3M4N5P6Q7R8S9U1",
            "agent_id": "root",
            "call_id": "call_01HZX8Y2K3M4N5P6Q7R8S9W3",
            "name": "host_result",
            "input": null,
            "context": {},
        }))
        .unwrap()
    }

    #[test]
    fn pinned_contract_digest_matches_the_canonical_schema() {
        assert_eq!(&*hand_contract_digest(), HAND_CONTRACT_DIGEST.trim());
        assert!(!HAND_CONTRACT_SCHEMA_JSON.contains("protocol_version"));
    }

    #[test]
    fn external_response_wire_bound_contains_both_terminal_projections() {
        let response = external_response(
            "x".repeat(crate::MAX_TOOL_TERMINAL_INLINE_BYTES),
            Value::String("y".repeat(crate::MAX_TOOL_TERMINAL_INLINE_BYTES - 2)),
        );
        assert!(external_tool_response_inline_fits(&response));
        assert!(external_tool_response_wire_fits(&response));

        let escaped = external_response(
            "\u{0001}".repeat(crate::MAX_TOOL_TERMINAL_INLINE_BYTES),
            Value::Null,
        );
        assert!(external_tool_response_inline_fits(&escaped));
        assert!(external_tool_response_wire_fits(&escaped));

        let mut value = serde_json::to_value(&escaped).unwrap();
        value["result_metadata"] = serde_json::json!({
            "oversized": "z".repeat(crate::MAX_EXTERNAL_TOOL_RESPONSE_BYTES),
        });
        let oversized: ExternalToolCallResponse = serde_json::from_value(value).unwrap();
        assert!(!external_tool_response_wire_fits(&oversized));
    }

    #[test]
    fn external_request_wire_bound_has_exact_cross_runtime_semantics() {
        let mut realistic = external_request();
        realistic.input = Value::String("i".repeat(crate::MAX_EXTERNAL_TOOL_INPUT_BYTES - 2));
        realistic.context.insert(
            "message.metadata".into(),
            "m".repeat(crate::MAX_MESSAGE_REQUEST_BYTES),
        );
        assert!(external_tool_request_wire_fits(&realistic));

        let mut exact = external_request();
        exact.context.insert("padding".into(), String::new());
        let base = external_tool_request_wire_bytes(&exact).unwrap();
        exact.context.insert(
            "padding".into(),
            "x".repeat(crate::MAX_EXTERNAL_TOOL_REQUEST_BYTES - base),
        );
        assert_eq!(
            external_tool_request_wire_bytes(&exact).unwrap(),
            crate::MAX_EXTERNAL_TOOL_REQUEST_BYTES
        );
        assert!(external_tool_request_wire_fits(&exact));
        exact.context.get_mut("padding").unwrap().push('x');
        assert_eq!(
            external_tool_request_wire_bytes(&exact).unwrap(),
            crate::MAX_EXTERNAL_TOOL_REQUEST_BYTES + 1
        );
        assert!(!external_tool_request_wire_fits(&exact));
    }
}
