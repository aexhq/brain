//! The private commit phase behind `session.output(schema)`.
//!
//! The ordinary agent never sees the schema. This module makes one constrained provider call
//! after work has finished, validates the candidate locally, and permits one isolated repair.
//! Neither control instruction, candidate failure, nor repair context is returned as model
//! history; the caller journals only the final validated value.

use crate::config::{Dialect, SealedPrefix, SessionConfig};
use crate::message::{ContentBlock, Message, Role, StopReason, Usage};
use crate::provider::{Accumulator, OutputControl, OutputMode, Provider};
use crate::{BrainError, Result, Shared};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub const MAX_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_CONTROL_CANDIDATE_CHARS: usize = 24 * 1024;
const MAX_ISSUES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword: Option<String>,
}

#[derive(Debug)]
pub struct CommitSuccess {
    pub value: Value,
    pub usage: Usage,
}

#[derive(Debug)]
pub struct CommitFailure {
    pub error: BrainError,
    pub issues: Vec<ValidationIssue>,
    pub usage: Usage,
}

pub struct CommitContext {
    pub provider: Arc<dyn Provider>,
    pub prefix: Shared<SealedPrefix>,
    pub session: SessionConfig,
    pub history: Vec<Message>,
    pub model_permits: Arc<Semaphore>,
    pub cancel: CancellationToken,
}

#[derive(Debug)]
struct Candidate {
    raw: String,
    value: Option<Value>,
    parse_error: Option<String>,
    usage: Usage,
}

/// SHA-256 over RFC 8785 canonical JSON, lower-case hexadecimal.
pub fn jcs_sha256<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    let canonical = serde_jcs::to_vec(value)?;
    Ok(hex::encode(Sha256::digest(canonical)))
}

/// Validate the caller's schema before admission, so a schema mistake never starts model work.
pub fn validate_schema(schema: &Value, claimed_hash: &str) -> Result<()> {
    let bytes = serde_json::to_vec(schema)?;
    if bytes.len() > MAX_SCHEMA_BYTES {
        return Err(BrainError::OutputSchema(format!(
            "schema is {} bytes; maximum is {MAX_SCHEMA_BYTES}",
            bytes.len()
        )));
    }
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(BrainError::OutputSchema(
            "the output schema must have an object root".into(),
        ));
    }
    reject_remote_refs(schema)?;
    jsonschema::meta::validate(schema)
        .map_err(|error| BrainError::OutputSchema(error.to_string()))?;
    jsonschema::draft202012::new(schema)
        .map_err(|error| BrainError::OutputSchema(error.to_string()))?;
    let actual = jcs_sha256(schema)?;
    if actual != claimed_hash {
        return Err(BrainError::OutputSchema(format!(
            "schema_hash mismatch: expected {actual}"
        )));
    }
    Ok(())
}

fn reject_remote_refs(value: &Value) -> Result<()> {
    match value {
        Value::Object(object) => {
            for keyword in ["$ref", "$dynamicRef", "$recursiveRef"] {
                if let Some(reference) = object.get(keyword).and_then(Value::as_str)
                    && !reference.starts_with('#')
                {
                    return Err(BrainError::OutputSchema(format!(
                        "remote {keyword} values are not supported"
                    )));
                }
            }
            for child in object.values() {
                reject_remote_refs(child)?;
            }
        }
        Value::Array(array) => {
            for child in array {
                reject_remote_refs(child)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Prefer provider-native constrained output, use one forced internal tool for schemas the
/// provider cannot constrain, then make at most one isolated repair on invalid data.
pub async fn commit(
    mut ctx: CommitContext,
    schema: Value,
) -> std::result::Result<CommitSuccess, CommitFailure> {
    let candidate_answer = take_candidate_answer(&mut ctx.history);
    let instruction = base_instruction(&candidate_answer);
    let mut usage = Usage::default();

    let preferred_mode = preferred_output_mode(&schema, ctx.provider.dialect());
    let (mode, first) = match invoke(&ctx, &schema, &instruction, preferred_mode).await {
        Ok(candidate) => (preferred_mode, candidate),
        Err(error)
            if preferred_mode == OutputMode::Native && native_capability_rejected(&error) =>
        {
            match invoke(&ctx, &schema, &instruction, OutputMode::ForcedTool).await {
                Ok(candidate) => (OutputMode::ForcedTool, candidate),
                Err(error) => {
                    return Err(CommitFailure {
                        error,
                        issues: Vec::new(),
                        usage,
                    });
                }
            }
        }
        Err(error) => {
            return Err(CommitFailure {
                error,
                issues: Vec::new(),
                usage,
            });
        }
    };
    usage.merge(&first.usage);

    let first_issues = candidate_issues(&schema, &first);
    if first_issues.is_empty() {
        return Ok(CommitSuccess {
            value: first.value.expect("no issues means parsed JSON"),
            usage,
        });
    }

    let repair_instruction = repair_instruction(&candidate_answer, &first.raw, &first_issues);
    let repaired = match invoke(&ctx, &schema, &repair_instruction, mode).await {
        Ok(candidate) => candidate,
        Err(error) => {
            return Err(CommitFailure {
                error,
                issues: first_issues,
                usage,
            });
        }
    };
    usage.merge(&repaired.usage);
    let repaired_issues = candidate_issues(&schema, &repaired);
    if repaired_issues.is_empty() {
        Ok(CommitSuccess {
            value: repaired.value.expect("no issues means parsed JSON"),
            usage,
        })
    } else {
        Err(CommitFailure {
            error: BrainError::OutputValidation(format!(
                "{} validation issue(s) remained after one repair",
                repaired_issues.len()
            )),
            issues: repaired_issues,
            usage,
        })
    }
}

async fn invoke(
    ctx: &CommitContext,
    schema: &Value,
    instruction: &str,
    mode: OutputMode,
) -> Result<Candidate> {
    let control = OutputControl {
        schema: provider_schema(schema, ctx.provider.dialect(), mode),
        instruction: instruction.to_string(),
    };
    let request = ctx.provider.build_output_request(
        &ctx.prefix,
        &ctx.history,
        &ctx.session.key,
        &ctx.session.base_url,
        &control,
        mode,
    )?;
    let _permit = tokio::select! {
        permit = ctx.model_permits.clone().acquire_owned() => {
            permit.map_err(|_| BrainError::Overloaded)?
        }
        () = ctx.cancel.cancelled() => return Err(BrainError::Cancelled),
    };
    let mut stream = tokio::select! {
        stream = ctx.provider.stream(request) => stream?,
        () = ctx.cancel.cancelled() => return Err(BrainError::Cancelled),
    };
    let mut accumulator = Accumulator::default();
    loop {
        let event = tokio::select! {
            event = stream.next() => event,
            () = ctx.cancel.cancelled() => return Err(BrainError::Cancelled),
        };
        match event {
            Some(Ok(event)) => accumulator.push(event),
            Some(Err(error)) => return Err(error),
            None => break,
        }
    }
    if !accumulator.saw_terminal {
        return Err(BrainError::Protocol(
            "provider output stream ended without a terminal event".into(),
        ));
    }
    let (message, stop, usage) = accumulator.finish()?;
    if stop == StopReason::Refusal {
        return Err(BrainError::OutputRefused(message_text(&message)));
    }
    candidate_from_message(message, usage, mode)
}

fn candidate_from_message(message: Message, usage: Usage, mode: OutputMode) -> Result<Candidate> {
    match mode {
        OutputMode::Native => {
            if message.tool_uses().next().is_some() {
                return Err(BrainError::Protocol(
                    "native output response unexpectedly called a tool".into(),
                ));
            }
            let raw = message_text(&message);
            let parsed = serde_json::from_str(raw.trim());
            let (value, parse_error) = match parsed {
                Ok(value) => (Some(value), None),
                Err(error) => (None, Some(format!("response was not valid JSON: {error}"))),
            };
            Ok(Candidate {
                raw,
                value,
                parse_error,
                usage,
            })
        }
        OutputMode::ForcedTool => {
            let mut calls = message.tool_uses();
            let Some((_, name, input)) = calls.next() else {
                let text = message_text(&message);
                return Err(if text.is_empty() {
                    BrainError::Protocol("forced output tool was not called".into())
                } else {
                    BrainError::OutputRefused(text)
                });
            };
            if name != "aex_output" || calls.next().is_some() {
                return Err(BrainError::Protocol(
                    "forced output response must contain exactly one aex_output call".into(),
                ));
            }
            Ok(Candidate {
                raw: serde_json::to_string(input)?,
                value: Some(input.clone()),
                parse_error: None,
                usage,
            })
        }
    }
}

fn candidate_issues(schema: &Value, candidate: &Candidate) -> Vec<ValidationIssue> {
    if let Some(message) = &candidate.parse_error {
        return vec![ValidationIssue {
            path: String::new(),
            message: message.clone(),
            keyword: None,
        }];
    }
    validation_issues(
        schema,
        candidate
            .value
            .as_ref()
            .expect("parsed candidate has a value"),
    )
}

fn validation_issues(schema: &Value, value: &Value) -> Vec<ValidationIssue> {
    let Ok(validator) = jsonschema::draft202012::new(schema) else {
        return vec![ValidationIssue {
            path: String::new(),
            message: "schema could not be compiled".into(),
            keyword: None,
        }];
    };
    validator
        .iter_errors(value)
        .take(MAX_ISSUES)
        .map(|error| ValidationIssue {
            path: error.instance_path().to_string(),
            message: error.to_string(),
            keyword: Some(error.kind().keyword().to_string()),
        })
        .collect()
}

fn provider_schema(schema: &Value, dialect: Dialect, mode: OutputMode) -> Value {
    let clean = clean_provider_schema(schema);
    if dialect == Dialect::AnthropicMessages && mode == OutputMode::Native {
        return anthropic_provider_schema(clean.clone()).unwrap_or(clean);
    }
    clean
}

fn clean_provider_schema(schema: &Value) -> Value {
    fn clean(value: &mut Value) {
        match value {
            Value::Object(object) => {
                object.remove("$schema");
                object.remove("$id");
                for child in object.values_mut() {
                    clean(child);
                }
            }
            Value::Array(array) => {
                for child in array {
                    clean(child);
                }
            }
            _ => {}
        }
    }
    let mut schema = schema.clone();
    clean(&mut schema);
    schema
}

fn preferred_output_mode(schema: &Value, dialect: Dialect) -> OutputMode {
    let clean = clean_provider_schema(schema);
    let native_compatible = match dialect {
        Dialect::AnthropicMessages => {
            objects_are_closed(&clean) && anthropic_provider_schema(clean).is_some()
        }
        Dialect::OpenAiChat => openai_native_compatible(&clean),
    };
    if native_compatible {
        OutputMode::Native
    } else {
        OutputMode::ForcedTool
    }
}

/// Strict structured-output dialects cannot represent an open-ended record. Falling back to a
/// non-strict forced tool preserves the caller's schema instead of silently closing the object.
fn objects_are_closed(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            if schema_includes_type(object, "object")
                && object.get("additionalProperties") != Some(&Value::Bool(false))
            {
                return false;
            }
            object.values().all(objects_are_closed)
        }
        Value::Array(array) => array.iter().all(objects_are_closed),
        _ => true,
    }
}

fn openai_native_compatible(value: &Value) -> bool {
    const UNSUPPORTED: [&str; 9] = [
        "allOf",
        "not",
        "dependentRequired",
        "dependentSchemas",
        "if",
        "then",
        "else",
        "oneOf",
        "default",
    ];

    match value {
        Value::Object(object) => {
            if UNSUPPORTED
                .iter()
                .any(|keyword| object.contains_key(*keyword))
            {
                return false;
            }
            if schema_includes_type(object, "object") {
                if object.get("additionalProperties") != Some(&Value::Bool(false)) {
                    return false;
                }
                let properties = match object.get("properties") {
                    Some(Value::Object(properties)) => properties,
                    None => {
                        return object.values().all(openai_native_compatible);
                    }
                    _ => return false,
                };
                let Some(required) = object.get("required").and_then(Value::as_array) else {
                    return properties.is_empty() && object.values().all(openai_native_compatible);
                };
                if properties
                    .keys()
                    .any(|name| !required.iter().any(|item| item.as_str() == Some(name)))
                {
                    return false;
                }
            }
            object.values().all(openai_native_compatible)
        }
        Value::Array(array) => array.iter().all(openai_native_compatible),
        _ => true,
    }
}

fn schema_includes_type(object: &Map<String, Value>, expected: &str) -> bool {
    match object.get("type") {
        Some(Value::String(actual)) => actual == expected,
        Some(Value::Array(types)) => types.iter().any(|value| value.as_str() == Some(expected)),
        _ => false,
    }
}

/// Match Anthropic's documented SDK transformation: retain the grammar-safe structural subset,
/// move unsupported constraints into descriptions, and keep local validation on the untouched
/// caller schema. `None` means the schema node has no representation in the native subset.
fn anthropic_provider_schema(value: Value) -> Option<Value> {
    const STRING_FORMATS: [&str; 10] = [
        "date-time",
        "time",
        "date",
        "duration",
        "email",
        "hostname",
        "uri",
        "ipv4",
        "ipv6",
        "uuid",
    ];

    let Value::Object(mut source) = value else {
        return None;
    };
    source.remove("$schema");
    source.remove("$id");

    let mut strict = Map::new();
    if let Some(reference) = source.remove("$ref") {
        strict.insert("$ref".into(), reference);
        return Some(Value::Object(strict));
    }
    if let Some(Value::Object(definitions)) = source.remove("$defs") {
        let transformed = definitions
            .into_iter()
            .map(|(name, schema)| Some((name, anthropic_provider_schema(schema)?)))
            .collect::<Option<Map<_, _>>>()?;
        strict.insert("$defs".into(), Value::Object(transformed));
    }

    let schema_type = source.remove("type");
    let any_of = source.remove("anyOf");
    let one_of = source.remove("oneOf");
    let all_of = source.remove("allOf");
    if let Some(Value::Array(variants)) = any_of.or(one_of) {
        let transformed = variants
            .into_iter()
            .map(anthropic_provider_schema)
            .collect::<Option<Vec<_>>>()?;
        strict.insert("anyOf".into(), Value::Array(transformed));
    } else if let Some(Value::Array(entries)) = all_of {
        let transformed = entries
            .into_iter()
            .map(anthropic_provider_schema)
            .collect::<Option<Vec<_>>>()?;
        strict.insert("allOf".into(), Value::Array(transformed));
    } else {
        strict.insert("type".into(), schema_type.clone()?);
    }

    if let Some(description) = source.remove("description") {
        strict.insert("description".into(), description);
    }
    if let Some(title) = source.remove("title") {
        strict.insert("title".into(), title);
    }

    match schema_type.as_ref().and_then(Value::as_str) {
        Some("object") => {
            let properties = match source.remove("properties") {
                Some(Value::Object(properties)) => properties,
                None => Map::new(),
                _ => return None,
            };
            let transformed = properties
                .into_iter()
                .map(|(name, schema)| Some((name, anthropic_provider_schema(schema)?)))
                .collect::<Option<Map<_, _>>>()?;
            strict.insert("properties".into(), Value::Object(transformed));
            source.remove("additionalProperties");
            strict.insert("additionalProperties".into(), Value::Bool(false));
            if let Some(required) = source.remove("required") {
                strict.insert("required".into(), required);
            }
        }
        Some("string") => {
            if let Some(format) = source.remove("format") {
                if format
                    .as_str()
                    .is_some_and(|value| STRING_FORMATS.contains(&value))
                {
                    strict.insert("format".into(), format);
                } else {
                    source.insert("format".into(), format);
                }
            }
        }
        Some("array") => {
            if let Some(items) = source.remove("items") {
                strict.insert("items".into(), anthropic_provider_schema(items)?);
            }
            if let Some(min_items) = source.remove("minItems") {
                if min_items.as_u64().is_some_and(|value| value <= 1) {
                    strict.insert("minItems".into(), min_items);
                } else {
                    source.insert("minItems".into(), min_items);
                }
            }
        }
        _ => {}
    }

    if !source.is_empty() {
        let constraints = source
            .iter()
            .map(|(name, value)| {
                format!(
                    "{name}: {}",
                    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = format!("{{{constraints}}}");
        let description = strict
            .get("description")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map_or(suffix.clone(), |current| format!("{current}\n\n{suffix}"));
        strict.insert("description".into(), Value::String(description));
    }
    Some(Value::Object(strict))
}

fn take_candidate_answer(history: &mut Vec<Message>) -> String {
    let Some(last) = history.last() else {
        return String::new();
    };
    if last.role != Role::Assistant {
        return String::new();
    }
    let message = history.pop().expect("checked last");
    truncate_chars(message_text(&message), MAX_CONTROL_CANDIDATE_CHARS)
}

fn message_text(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.clone()),
            ContentBlock::Output { value, .. } => serde_json::to_string(value).ok(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

fn base_instruction(candidate: &str) -> String {
    let mut instruction = String::from(
        "Aex private output commit. Return the answer in the required structured format. \
         Preserve facts already established in the session. Do not perform new work and do not \
         call any tool except the forced aex_output tool if it is the only tool offered.",
    );
    if !candidate.is_empty() {
        instruction.push_str("\n\nCandidate answer from the completed work phase:\n");
        instruction.push_str(candidate);
    }
    instruction
}

fn repair_instruction(candidate_answer: &str, invalid: &str, issues: &[ValidationIssue]) -> String {
    let mut instruction = base_instruction(candidate_answer);
    instruction.push_str(
        "\n\nThe previous private output candidate failed validation. Correct only its shape or \
         values needed to satisfy the required schema; do not add unsupported facts.\nCandidate:\n",
    );
    instruction.push_str(&truncate_chars(
        invalid.to_string(),
        MAX_CONTROL_CANDIDATE_CHARS,
    ));
    instruction.push_str("\nValidation issues:\n");
    instruction.push_str(
        &serde_json::to_string(issues)
            .unwrap_or_else(|_| "[{\"message\":\"validation failed\"}]".into()),
    );
    instruction
}

fn truncate_chars(value: String, max: usize) -> String {
    if value.chars().count() <= max {
        return value;
    }
    value.chars().take(max).collect()
}

fn native_capability_rejected(error: &BrainError) -> bool {
    matches!(
        error,
        BrainError::ProviderStatus {
            status: 400 | 404 | 415 | 422,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentDef, Dialect, ProviderKey};
    use crate::provider::fake::{FakeProvider, Scripted};
    use crate::provider::{ModelRequest, ProviderEvent};
    use futures_util::stream::BoxStream;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Debug)]
    struct NativeRejectingProvider {
        inner: Arc<FakeProvider>,
        rejected: AtomicU64,
    }

    #[async_trait::async_trait]
    impl Provider for NativeRejectingProvider {
        fn dialect(&self) -> Dialect {
            self.inner.dialect()
        }

        fn build_request(
            &self,
            prefix: &SealedPrefix,
            history: &[Message],
            key: &ProviderKey,
            base_url: &str,
        ) -> Result<ModelRequest> {
            self.inner.build_request(prefix, history, key, base_url)
        }

        fn build_output_request(
            &self,
            prefix: &SealedPrefix,
            history: &[Message],
            key: &ProviderKey,
            base_url: &str,
            control: &OutputControl,
            mode: OutputMode,
        ) -> Result<ModelRequest> {
            self.inner
                .build_output_request(prefix, history, key, base_url, control, mode)
        }

        async fn stream(
            &self,
            request: ModelRequest,
        ) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
            let body: Value = serde_json::from_slice(&request.body)?;
            if body.get("output_config").is_some() {
                self.rejected.fetch_add(1, Ordering::SeqCst);
                return Err(BrainError::ProviderStatus {
                    status: 400,
                    body: "structured output is not supported".into(),
                });
            }
            self.inner.stream(request).await
        }
    }

    #[test]
    fn schema_hash_is_canonical_and_verified() {
        let a = json!({"type":"object","properties":{"b":{"type":"string"},"a":{"type":"number"}}});
        let b = json!({"properties":{"a":{"type":"number"},"b":{"type":"string"}},"type":"object"});
        let hash = jcs_sha256(&a).unwrap();
        assert_eq!(hash, jcs_sha256(&b).unwrap());
        validate_schema(&b, &hash).unwrap();
    }

    #[test]
    fn schema_mismatch_and_remote_refs_fail_before_a_model_call() {
        let schema =
            json!({"type":"object","properties":{"x":{"$ref":"https://example.test/schema"}}});
        let error = validate_schema(&schema, &"0".repeat(64)).unwrap_err();
        assert!(matches!(error, BrainError::OutputSchema(_)));

        let schema = json!({"type":"object","properties":{"x":{"$dynamicRef":"https://example.test/schema"}}});
        let error = validate_schema(&schema, &"0".repeat(64)).unwrap_err();
        assert!(matches!(error, BrainError::OutputSchema(_)));

        let schema = json!({"type":"object"});
        let error = validate_schema(&schema, &"0".repeat(64)).unwrap_err();
        assert!(error.to_string().contains("schema_hash mismatch"));
    }

    #[test]
    fn validation_reports_json_pointer_without_coercion() {
        let schema = json!({
            "type":"object",
            "additionalProperties":false,
            "required":["count"],
            "properties":{"count":{"type":"number"}}
        });
        let issues = validation_issues(&schema, &json!({"count":"42"}));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "/count");
        assert_eq!(issues[0].keyword.as_deref(), Some("type"));
    }

    #[test]
    fn provider_modes_preserve_optional_fields_and_open_records() {
        let optional = json!({
            "type":"object",
            "additionalProperties":false,
            "required":["answer"],
            "properties":{
                "answer":{"type":"string"},
                "note":{"type":"string"}
            }
        });
        assert_eq!(
            preferred_output_mode(&optional, Dialect::AnthropicMessages),
            OutputMode::Native
        );
        assert_eq!(
            preferred_output_mode(&optional, Dialect::OpenAiChat),
            OutputMode::ForcedTool
        );

        let required = json!({
            "type":"object",
            "additionalProperties":false,
            "required":["answer"],
            "properties":{"answer":{"type":"string"}}
        });
        assert_eq!(
            preferred_output_mode(&required, Dialect::OpenAiChat),
            OutputMode::Native
        );

        let record = json!({
            "type":"object",
            "additionalProperties":false,
            "required":["values"],
            "properties":{
                "values":{
                    "type":"object",
                    "additionalProperties":{"type":"number"}
                }
            }
        });
        assert_eq!(
            preferred_output_mode(&record, Dialect::AnthropicMessages),
            OutputMode::ForcedTool
        );
        assert_eq!(
            preferred_output_mode(&record, Dialect::OpenAiChat),
            OutputMode::ForcedTool
        );
    }

    #[test]
    fn anthropic_native_schema_describes_unsupported_constraints() {
        let original = json!({
            "$schema":"https://json-schema.org/draft/2020-12/schema",
            "type":"object",
            "additionalProperties":false,
            "required":["score","tag","email"],
            "properties":{
                "score":{"type":"number","minimum":1,"maximum":5},
                "tag":{"type":"string","minLength":3,"format":"custom-tag"},
                "email":{"type":"string","format":"email"}
            }
        });
        let transformed =
            provider_schema(&original, Dialect::AnthropicMessages, OutputMode::Native);
        assert!(transformed.get("$schema").is_none());
        assert!(transformed["properties"]["score"].get("minimum").is_none());
        assert!(
            transformed["properties"]["score"]["description"]
                .as_str()
                .unwrap()
                .contains("minimum: 1")
        );
        assert!(
            transformed["properties"]["tag"]["description"]
                .as_str()
                .unwrap()
                .contains("format: \"custom-tag\"")
        );
        assert_eq!(transformed["properties"]["email"]["format"], "email");

        let issues = validation_issues(&original, &json!({"score":0,"tag":"x","email":"a@b.com"}));
        assert!(
            issues
                .iter()
                .any(|issue| issue.keyword.as_deref() == Some("minimum"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.keyword.as_deref() == Some("minLength"))
        );
    }

    #[tokio::test]
    async fn openai_optional_schema_starts_with_the_non_strict_forced_tool() {
        let fake = Arc::new(FakeProvider::new(Dialect::OpenAiChat));
        fake.script([Scripted::tool("aex_output", json!({"answer":"done"}))]);
        let prefix = AgentDef::new("system", "fake", Dialect::OpenAiChat).seal();
        let session = SessionConfig::new(
            prefix.clone(),
            ProviderKey::new("sk-fake"),
            "https://example.test",
        );
        let result = commit(
            CommitContext {
                provider: fake.clone(),
                prefix,
                session,
                history: vec![Message::user_text("Answer")],
                model_permits: Arc::new(Semaphore::new(1)),
                cancel: CancellationToken::new(),
            },
            json!({
                "type":"object",
                "additionalProperties":false,
                "required":["answer"],
                "properties":{
                    "answer":{"type":"string"},
                    "note":{"type":"string"}
                }
            }),
        )
        .await
        .unwrap();
        assert_eq!(result.value, json!({"answer":"done"}));
        fake.assert_drained(1, "optional OpenAI output").unwrap();
        assert_eq!(fake.arrivals.lock().unwrap()[0].tools_offered, 1);
    }

    #[tokio::test]
    async fn native_capability_rejection_uses_one_forced_terminal_tool() {
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        fake.script([Scripted::tool("aex_output", json!({"ok":true}))]);
        let provider = Arc::new(NativeRejectingProvider {
            inner: fake.clone(),
            rejected: AtomicU64::new(0),
        });
        let prefix = AgentDef::new("system", "fake", Dialect::AnthropicMessages).seal();
        let session = SessionConfig::new(
            prefix.clone(),
            ProviderKey::new("sk-fake"),
            "https://example.test",
        );
        let schema = json!({
            "type":"object",
            "additionalProperties":false,
            "required":["ok"],
            "properties":{"ok":{"type":"boolean"}}
        });
        let result = commit(
            CommitContext {
                provider: provider.clone(),
                prefix,
                session,
                history: vec![
                    Message::user_text("Do the work"),
                    Message::assistant(vec![ContentBlock::text("done")]),
                ],
                model_permits: Arc::new(Semaphore::new(1)),
                cancel: CancellationToken::new(),
            },
            schema,
        )
        .await
        .unwrap();
        assert_eq!(result.value, json!({"ok":true}));
        assert_eq!(provider.rejected.load(Ordering::SeqCst), 1);
        fake.assert_drained(1, "forced output fallback").unwrap();
        let arrivals = fake.arrivals.lock().unwrap();
        assert_eq!(arrivals[0].tools_offered, 1);
    }

    #[tokio::test]
    async fn invalid_output_repairs_once_then_fails() {
        let fake = Arc::new(FakeProvider::new(Dialect::AnthropicMessages));
        fake.script([
            Scripted::Text(r#"{"count":"one"}"#.into()),
            Scripted::Text(r#"{"count":"still wrong"}"#.into()),
        ]);
        let prefix = AgentDef::new("system", "fake", Dialect::AnthropicMessages).seal();
        let session = SessionConfig::new(
            prefix.clone(),
            ProviderKey::new("sk-fake"),
            "https://example.test",
        );
        let failure = commit(
            CommitContext {
                provider: fake.clone(),
                prefix,
                session,
                history: vec![Message::user_text("Count")],
                model_permits: Arc::new(Semaphore::new(1)),
                cancel: CancellationToken::new(),
            },
            json!({
                "type":"object",
                "additionalProperties":false,
                "required":["count"],
                "properties":{"count":{"type":"number"}}
            }),
        )
        .await
        .unwrap_err();
        assert!(matches!(failure.error, BrainError::OutputValidation(_)));
        assert_eq!(failure.issues[0].path, "/count");
        fake.assert_drained(2, "one repair only").unwrap();
    }
}
