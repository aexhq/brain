//! MCP wire building blocks: the JSON-RPC envelope, the 2026-07-28 `_meta` carriage, header
//! mirroring (`Mcp-Method` / `Mcp-Name` / `Mcp-Param-*`), the Base64 sentinel value encoding,
//! and `x-mcp-header` schema validation. Everything here is pure; the transport lives in
//! [`super::client`].
//!
//! Spec: modelcontextprotocol.io 2026-07-28 (stateless core, SEP-2243 headers, SEP-2322 MRTR)
//! and the 2025-03-26..2025-11-25 Streamable HTTP lineage for the legacy adapter.

use serde_json::{Value, json};
use std::collections::HashSet;

/// The stateless revision we speak natively.
pub const V2_VERSION: &str = "2026-07-28";
/// What the legacy adapter offers in `initialize`.
pub const LEGACY_OFFER: &str = "2025-06-18";
/// Initialization-era revisions the legacy adapter accepts from a server.
pub const LEGACY_ACCEPTED: [&str; 3] = ["2025-03-26", "2025-06-18", "2025-11-25"];

pub const CLIENT_NAME: &str = "aex-brain";

// JSON-RPC error codes the 2026-07-28 revision defines. A 400 whose body carries one of
// these came from a MODERN server -- the backward-compat probe must NOT fall back on them.
pub const HEADER_MISMATCH: i64 = -32020;
pub const MISSING_CLIENT_CAPABILITY: i64 = -32021;
pub const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;
pub const METHOD_NOT_FOUND: i64 = -32601;

pub fn is_modern_error(code: i64) -> bool {
    matches!(
        code,
        HEADER_MISMATCH | MISSING_CLIENT_CAPABILITY | UNSUPPORTED_PROTOCOL_VERSION
    )
}

/// A JSON-RPC request body. v2 requests carry the protocol version, client capabilities and
/// client identity in `params._meta` on EVERY request (there is no handshake to remember).
pub fn request(id: u64, method: &str, mut params: Value, v2: bool) -> Value {
    if !params.is_object() {
        params = json!({});
    }
    if v2 {
        let meta = params
            .as_object_mut()
            .expect("params is an object")
            .entry("_meta")
            .or_insert_with(|| json!({}));
        if let Some(m) = meta.as_object_mut() {
            m.insert(
                "io.modelcontextprotocol/protocolVersion".into(),
                json!(V2_VERSION),
            );
            m.insert(
                "io.modelcontextprotocol/clientCapabilities".into(),
                json!({}),
            );
            m.insert(
                "io.modelcontextprotocol/clientInfo".into(),
                json!({"name": CLIENT_NAME, "version": env!("CARGO_PKG_VERSION")}),
            );
        }
    }
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// A JSON-RPC notification body (no id, no reply).
pub fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

/// One parsed JSON-RPC reply.
#[derive(Debug)]
pub enum Reply {
    Result(Value),
    Error {
        code: i64,
        message: String,
        data: Option<Value>,
    },
}

/// Parses a JSON-RPC message as the reply to request `expect_id`. Messages with a `method`
/// are notifications or server-initiated requests, not replies -- `Ok(None)`.
pub fn parse_reply(msg: &Value, expect_id: u64) -> Result<Option<Reply>, String> {
    if msg.get("method").is_some() {
        return Ok(None);
    }
    let id = msg.get("id").and_then(|v| v.as_u64());
    if id != Some(expect_id) {
        return Err(format!(
            "reply id {:?} does not match request id {expect_id}",
            msg.get("id")
        ));
    }
    if let Some(err) = msg.get("error") {
        return Ok(Some(Reply::Error {
            code: err.get("code").and_then(|c| c.as_i64()).unwrap_or(0),
            message: err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("(no message)")
                .to_string(),
            data: err.get("data").cloned(),
        }));
    }
    match msg.get("result") {
        Some(r) => Ok(Some(Reply::Result(r.clone()))),
        None => Err("reply carries neither result nor error".into()),
    }
}

/// How a v2 result resolves. Results from pre-2026 servers omit `resultType`; the spec says
/// clients MUST treat that as `"complete"`.
#[derive(Debug, PartialEq, Eq)]
pub enum ResultKind {
    Complete,
    /// MRTR: the server wants sampling/elicitation/roots input. We declare zero client
    /// capabilities, so this is always surfaced as a structured tool failure, never honoured.
    InputRequired,
}

pub fn result_kind(result: &Value) -> ResultKind {
    match result.get("resultType").and_then(|v| v.as_str()) {
        Some("input_required") => ResultKind::InputRequired,
        _ => ResultKind::Complete,
    }
}

// ---------------------------------------------------------------------------------------------
// Header value encoding (SEP-2243)
// ---------------------------------------------------------------------------------------------

const SENTINEL_PREFIX: &str = "=?base64?";
const SENTINEL_SUFFIX: &str = "?=";

fn header_safe(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with([' ', '\t'])
        && !s.ends_with([' ', '\t'])
        && s.bytes()
            .all(|b| b == b' ' || b == b'\t' || (0x21..=0x7e).contains(&b))
        && !(s.starts_with(SENTINEL_PREFIX) && s.ends_with(SENTINEL_SUFFIX))
}

/// Encodes a value for an `Mcp-Name` / `Mcp-Param-*` header: as-is when header-safe, Base64
/// sentinel otherwise (including plain values that merely LOOK like the sentinel).
pub fn header_value(raw: &str) -> String {
    if header_safe(raw) {
        raw.to_string()
    } else {
        use base64::Engine;
        format!(
            "{SENTINEL_PREFIX}{}{SENTINEL_SUFFIX}",
            base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
        )
    }
}

// ---------------------------------------------------------------------------------------------
// x-mcp-header (SEP-2243 custom headers from tool parameters)
// ---------------------------------------------------------------------------------------------

fn tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// One validated annotation: the header-name part and the `properties`-only path to the value.
#[derive(Debug, Clone, PartialEq)]
pub struct HeaderParam {
    pub header: String,
    pub path: Vec<String>,
}

/// Validates every `x-mcp-header` annotation in a tool's input schema per the spec's
/// constraints. `Err` means the TOOL DEFINITION is invalid and the client MUST exclude the
/// tool. `Ok` carries the annotations to mirror at call time (possibly none).
pub fn validate_tool_headers(input_schema: &Value) -> Result<Vec<HeaderParam>, String> {
    let mut found = Vec::new();
    let mut names = HashSet::new();
    collect(input_schema, &mut Vec::new(), true, &mut found)?;
    for p in &found {
        if p.header.is_empty() {
            return Err("x-mcp-header value must not be empty".into());
        }
        if !p.header.bytes().all(tchar) {
            return Err(format!(
                "x-mcp-header {:?} is not a valid HTTP field-name token",
                p.header
            ));
        }
        if !names.insert(p.header.to_ascii_lowercase()) {
            return Err(format!(
                "x-mcp-header {:?} is not case-insensitively unique",
                p.header
            ));
        }
    }
    return Ok(found);

    /// Walks the whole schema. `reachable` is true only along chains made purely of
    /// `properties` keys from the root; an annotation anywhere else invalidates the tool.
    fn collect(
        node: &Value,
        path: &mut Vec<String>,
        reachable: bool,
        out: &mut Vec<HeaderParam>,
    ) -> Result<(), String> {
        let Some(obj) = node.as_object() else {
            return Ok(());
        };
        if let Some(h) = obj.get("x-mcp-header") {
            if !reachable {
                return Err(
                    "x-mcp-header on a property not statically reachable via properties chains"
                        .into(),
                );
            }
            let Some(h) = h.as_str() else {
                return Err("x-mcp-header must be a string".into());
            };
            match obj.get("type").and_then(|t| t.as_str()) {
                Some("string") | Some("integer") | Some("boolean") => {}
                other => {
                    return Err(format!(
                        "x-mcp-header {h:?} on type {other:?}; only string/integer/boolean are permitted"
                    ));
                }
            }
            out.push(HeaderParam {
                header: h.to_string(),
                path: path.clone(),
            });
        }
        for (k, v) in obj {
            if k == "properties" && v.is_object() {
                for (prop, sub) in v.as_object().expect("checked") {
                    path.push(prop.clone());
                    collect(sub, path, reachable, out)?;
                    path.pop();
                }
            } else if v.is_object() || v.is_array() {
                // Any other keyword (items, oneOf, $ref bodies, if/then...) breaks
                // reachability for everything beneath it.
                match v {
                    Value::Array(items) => {
                        for item in items {
                            collect(item, path, false, out)?;
                        }
                    }
                    other => collect(other, path, false, out)?,
                }
            }
        }
        Ok(())
    }
}

/// Extracts the `Mcp-Param-{Name}` headers for one call: reads each annotated path from the
/// arguments, omitting absent/null values, converting and encoding per the spec. A value that
/// cannot be represented (float, unsafe integer, non-primitive) fails the CALL -- sending the
/// body value without its mirror header is a server-side rejection anyway.
pub fn param_headers(
    params: &[HeaderParam],
    args: &Value,
) -> Result<Vec<(String, String)>, String> {
    const SAFE: i64 = 9_007_199_254_740_991; // 2^53 - 1
    let mut out = Vec::new();
    for p in params {
        let mut node = args;
        let mut present = true;
        for step in &p.path {
            match node.get(step) {
                Some(v) => node = v,
                None => {
                    present = false;
                    break;
                }
            }
        }
        if !present || node.is_null() {
            continue;
        }
        let s = match node {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => match n.as_i64() {
                Some(i) if (-SAFE..=SAFE).contains(&i) => i.to_string(),
                _ => {
                    return Err(format!(
                        "argument {} for header {} is not a safe integer",
                        p.path.join("."),
                        p.header
                    ));
                }
            },
            _ => {
                return Err(format!(
                    "argument {} for header {} is not a primitive",
                    p.path.join("."),
                    p.header
                ));
            }
        };
        out.push((format!("Mcp-Param-{}", p.header), header_value(&s)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_request_carries_meta_on_every_call() {
        let r = request(7, "tools/call", json!({"name": "x", "arguments": {}}), true);
        assert_eq!(r["id"], 7);
        assert_eq!(
            r["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"],
            V2_VERSION
        );
        assert!(r["params"]["_meta"]["io.modelcontextprotocol/clientCapabilities"].is_object());
        // Legacy requests do NOT carry the v2 meta.
        let l = request(8, "tools/list", json!({}), false);
        assert!(l["params"].get("_meta").is_none());
    }

    #[test]
    fn reply_parsing_matches_ids_and_skips_notifications() {
        assert!(
            parse_reply(
                &json!({"jsonrpc":"2.0","method":"notifications/progress"}),
                1
            )
            .unwrap()
            .is_none()
        );
        let ok = parse_reply(&json!({"jsonrpc":"2.0","id":1,"result":{"x":1}}), 1)
            .unwrap()
            .unwrap();
        assert!(matches!(ok, Reply::Result(_)));
        assert!(parse_reply(&json!({"jsonrpc":"2.0","id":2,"result":{}}), 1).is_err());
        let err = parse_reply(
            &json!({"jsonrpc":"2.0","id":1,"error":{"code":-32022,"message":"nope","data":{"supported":["2026-07-28"]}}}),
            1,
        )
        .unwrap()
        .unwrap();
        match err {
            Reply::Error { code, data, .. } => {
                assert_eq!(code, UNSUPPORTED_PROTOCOL_VERSION);
                assert!(is_modern_error(code));
                assert!(data.unwrap()["supported"].is_array());
            }
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn result_kind_treats_missing_result_type_as_complete() {
        assert_eq!(result_kind(&json!({"content": []})), ResultKind::Complete);
        assert_eq!(
            result_kind(&json!({"resultType": "complete"})),
            ResultKind::Complete
        );
        assert_eq!(
            result_kind(&json!({"resultType": "input_required", "inputRequests": []})),
            ResultKind::InputRequired
        );
    }

    #[test]
    fn header_value_encoding_matches_the_spec_examples() {
        assert_eq!(header_value("us-west1"), "us-west1");
        assert_eq!(
            header_value("Hello, 世界"),
            "=?base64?SGVsbG8sIOS4lueVjA==?="
        );
        assert_eq!(header_value(" padded "), "=?base64?IHBhZGRlZCA=?=");
        assert_eq!(header_value("line1\nline2"), "=?base64?bGluZTEKbGluZTI=?=");
        assert_eq!(
            header_value("=?base64?literal?="),
            "=?base64?PT9iYXNlNjQ/bGl0ZXJhbD89?="
        );
    }

    fn spanner_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "region": {"type": "string", "x-mcp-header": "Region"},
                "query": {"type": "string"}
            },
            "required": ["region", "query"]
        })
    }

    #[test]
    fn x_mcp_header_extraction_mirrors_annotated_params() {
        let params = validate_tool_headers(&spanner_schema()).unwrap();
        assert_eq!(params.len(), 1);
        let hs =
            param_headers(&params, &json!({"region": "us-west1", "query": "SELECT 1"})).unwrap();
        assert_eq!(hs, vec![("Mcp-Param-Region".into(), "us-west1".into())]);
        // Absent and null values omit the header.
        assert!(
            param_headers(&params, &json!({"query": "x"}))
                .unwrap()
                .is_empty()
        );
        assert!(
            param_headers(&params, &json!({"region": null}))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn x_mcp_header_nested_properties_chains_are_reachable() {
        let schema = json!({
            "type": "object",
            "properties": {
                "opts": {
                    "type": "object",
                    "properties": {
                        "tenant": {"type": "string", "x-mcp-header": "Tenant"}
                    }
                }
            }
        });
        let params = validate_tool_headers(&schema).unwrap();
        assert_eq!(params[0].path, vec!["opts", "tenant"]);
        let hs = param_headers(&params, &json!({"opts": {"tenant": "acme"}})).unwrap();
        assert_eq!(hs[0].1, "acme");
    }

    #[test]
    fn x_mcp_header_invalid_annotations_invalidate_the_tool() {
        // Through an array keyword.
        assert!(
            validate_tool_headers(&json!({
                "type": "object",
                "properties": {"xs": {"type": "array", "items": {"type": "string", "x-mcp-header": "X"}}}
            }))
            .is_err()
        );
        // Through a composition keyword.
        assert!(
            validate_tool_headers(&json!({
                "oneOf": [{"type": "object", "properties": {"a": {"type": "string", "x-mcp-header": "A"}}}]
            }))
            .is_err()
        );
        // number type is not permitted.
        assert!(
            validate_tool_headers(&json!({
                "type": "object",
                "properties": {"n": {"type": "number", "x-mcp-header": "N"}}
            }))
            .is_err()
        );
        // Duplicate names differing only in case.
        assert!(
            validate_tool_headers(&json!({
                "type": "object",
                "properties": {
                    "a": {"type": "string", "x-mcp-header": "Region"},
                    "b": {"type": "string", "x-mcp-header": "region"}
                }
            }))
            .is_err()
        );
        // Invalid token syntax.
        assert!(
            validate_tool_headers(&json!({
                "type": "object",
                "properties": {"a": {"type": "string", "x-mcp-header": "bad name"}}
            }))
            .is_err()
        );
    }

    #[test]
    fn param_values_that_cannot_be_headers_fail_the_call() {
        let schema = json!({
            "type": "object",
            "properties": {"n": {"type": "integer", "x-mcp-header": "N"}}
        });
        let params = validate_tool_headers(&schema).unwrap();
        assert_eq!(
            param_headers(&params, &json!({"n": 42})).unwrap()[0].1,
            "42"
        );
        assert!(param_headers(&params, &json!({"n": 4.5})).is_err());
        assert!(param_headers(&params, &json!({"n": 9007199254740993_i64})).is_err());
        assert!(param_headers(&params, &json!({"n": {"deep": 1}})).is_err());
    }
}
