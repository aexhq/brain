//! Semantic, append-only model-context checkpoints.
//!
//! The immutable journal remains the audit source. The bounded model view follows the Codex/Pi
//! shape: previous summary + newly covered span + a recent complete-turn tail.

use base64::Engine as _;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::Arc;

use crate::config::{AgentDef, GenOpts, SessionConfig};
use crate::journal::{ContextPointerDoc, Entry, Record};
use crate::message::{ContentBlock, Message, StopReason};
use crate::provider::{Accumulator, Provider};
use crate::{BrainError, Result};

pub const DEFAULT_CONTEXT_SOFT_TOKENS: usize = 96 * 1024;
pub const DEFAULT_CONTEXT_HARD_TOKENS: usize = 112 * 1024;
pub const DEFAULT_CONTEXT_TAIL_TOKENS: usize = 24 * 1024;
pub const CONTEXT_CHUNK_BYTES: usize = 64 * 1024;
const SUMMARY_TOKEN_BUDGET: usize = 16 * 1024;
const CONTEXT_SAFETY_MIN_TOKENS: usize = 2 * 1024;
const COMPACTION_TOOL_RESULT_PREVIEW_BYTES: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudget {
    pub soft_tokens: usize,
    pub hard_tokens: usize,
    pub tail_tokens: usize,
    pub summary_tokens: usize,
}

/// Derive one immutable per-session request budget from the fully rendered stable prefix, the
/// declared model window, and the requested output reserve. Every UTF-8 byte is counted as a
/// possible token: this is deliberately stricter than prose heuristics because an arbitrary
/// compatible provider may tokenize code, UUIDs, base64, or multilingual input differently.
/// A further 10% (at least 2K) reserve covers provider-owned framing. The same hard input ceiling
/// must fit the semantic-compactor request, including its own instructions and summary output.
pub fn derive_context_budget(
    rendered_base: &serde_json::Value,
    context_window_tokens: usize,
    max_output_tokens: usize,
    host_soft_tokens: usize,
    host_hard_tokens: usize,
    host_tail_tokens: usize,
) -> Result<ContextBudget> {
    let stable_prefix_tokens = estimate_text_tokens(&serde_jcs::to_string(rendered_base)?);
    let safety_tokens = context_safety_tokens(context_window_tokens);
    let normal_fixed = stable_prefix_tokens
        .saturating_add(max_output_tokens)
        .saturating_add(safety_tokens);
    let normal_history = context_window_tokens.checked_sub(normal_fixed).ok_or_else(|| {
        BrainError::Invalid(format!(
            "the rendered model prefix ({stable_prefix_tokens} estimated tokens), requested output ({max_output_tokens}), and safety reserve ({safety_tokens}) do not fit the sealed {context_window_tokens}-token context window"
        ))
    })?;

    let summary_tokens = SUMMARY_TOKEN_BUDGET
        .min(context_window_tokens / 8)
        .max(1_024);
    let compactor_fixed = estimate_text_tokens(COMPACTION_INSTRUCTIONS)
        .saturating_add(summary_tokens)
        .saturating_add(safety_tokens)
        .saturating_add(512); // canonical JSON wrapper/roles
    let compactor_input = context_window_tokens
        .checked_sub(compactor_fixed)
        .ok_or_else(|| {
            BrainError::Invalid(format!(
                "semantic compaction plus its output reserve does not fit the sealed {context_window_tokens}-token context window"
            ))
        })?;

    let hard_tokens = normal_history.min(compactor_input).min(host_hard_tokens);
    if hard_tokens < 1_024 {
        return Err(BrainError::Invalid(
            "the sealed model context leaves fewer than 1024 tokens for conversation history"
                .into(),
        ));
    }
    let soft_tokens = host_soft_tokens.min(hard_tokens.saturating_mul(9) / 10);
    if soft_tokens == 0 || soft_tokens >= hard_tokens {
        return Err(BrainError::Invalid(
            "the derived context soft limit must be positive and below the hard limit".into(),
        ));
    }
    let tail_tokens = host_tail_tokens.min((soft_tokens / 4).max(1));
    Ok(ContextBudget {
        soft_tokens,
        hard_tokens,
        tail_tokens,
        summary_tokens,
    })
}

fn context_safety_tokens(context_window_tokens: usize) -> usize {
    CONTEXT_SAFETY_MIN_TOKENS.max(context_window_tokens / 10)
}

/// Pure last-mile preflight over the exact provider request body. Counting each body byte as a
/// token is intentionally conservative and, unlike a prose ratio, remains safe for arbitrary
/// byte-fallback tokenizers. Callers must run this before committing/sending an effect intent.
pub fn validate_model_request_budget(
    body_bytes: usize,
    max_output_tokens: usize,
    context_window_tokens: usize,
    purpose: &str,
) -> Result<()> {
    let safety_tokens = context_safety_tokens(context_window_tokens);
    let projected = body_bytes
        .saturating_add(max_output_tokens)
        .saturating_add(safety_tokens);
    if projected > context_window_tokens {
        return Err(BrainError::Protocol(format!(
            "{purpose} request needs at most {projected} conservative tokens ({body_bytes} exact body bytes, {max_output_tokens} output, {safety_tokens} safety) but the sealed context window is {context_window_tokens}"
        )));
    }
    Ok(())
}

pub const COMPACTION_INSTRUCTIONS: &str = r#"Produce a structured handoff summary for another agent that will continue the same session.
Preserve goals, constraints, user corrections, architectural decisions, completed and in-progress work,
unresolved failures, exact identifiers, exact tool-call/result pairings, file and durable-storage references,
and concrete next actions. Treat the previous summary as authoritative accumulated memory and merge it
without dropping material facts. Summarize only the NEW canonical span; do not summarize or repeat the
retained tail. Do not claim success where an operation failed or remains ambiguous. Output only the new
standalone summary, with enough exact detail to replace the previous summary."#;

#[derive(Debug, Clone)]
pub struct CompactionPlan {
    pub previous_summary: Option<String>,
    pub new_span: Vec<Message>,
    pub tail: Vec<Message>,
    pub retained_messages: u64,
    pub source_context_digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactionRequest {
    pub previous_summary: Option<String>,
    pub new_span: Vec<Message>,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub summary: String,
    pub provider: String,
    pub model: String,
    pub usage: crate::message::Usage,
}

pub struct CompactionModel<'a> {
    pub provider: Arc<dyn Provider>,
    pub session: &'a SessionConfig,
    pub provider_name: &'a str,
    pub outbound: &'a crate::outbound::Outbound,
    pub cancel: &'a tokio_util::sync::CancellationToken,
    pub header_timeout: std::time::Duration,
    pub idle_timeout: std::time::Duration,
    pub total_timeout: std::time::Duration,
    pub context_window_tokens: usize,
}

#[async_trait::async_trait]
pub trait CompactionPort: Send + Sync {
    /// Pure request construction/budget validation. The turn invokes this before journaling an
    /// intent so a deterministic context overflow can never masquerade as an ambiguous call.
    fn preflight(&self, _request: &CompactionRequest, _model: &CompactionModel<'_>) -> Result<()> {
        Ok(())
    }

    async fn compact(
        &self,
        request: CompactionRequest,
        model: CompactionModel<'_>,
    ) -> Result<CompactionResult>;
}

/// Generic Codex/Pi-style semantic compactor using the session's selected provider/model.
#[derive(Debug, Default)]
pub struct SameProviderCompactor;

#[async_trait::async_trait]
impl CompactionPort for SameProviderCompactor {
    fn preflight(&self, request: &CompactionRequest, model: &CompactionModel<'_>) -> Result<()> {
        let wire = build_compaction_wire(request, model)?;
        validate_model_request_budget(
            wire.body_len(),
            request.max_output_tokens as usize,
            model.context_window_tokens,
            "semantic compaction",
        )
    }

    async fn compact(
        &self,
        request: CompactionRequest,
        model: CompactionModel<'_>,
    ) -> Result<CompactionResult> {
        if request.new_span.is_empty() {
            return Err(BrainError::Protocol(
                "compaction has no newly covered span".into(),
            ));
        }
        let wire = build_compaction_wire(&request, &model)?;
        validate_model_request_budget(
            wire.body_len(),
            request.max_output_tokens as usize,
            model.context_window_tokens,
            "semantic compaction",
        )?;
        let mut total = std::pin::pin!(tokio::time::sleep(model.total_timeout));
        let mut stream = tokio::select! {
            stream = tokio::time::timeout(
                model.header_timeout,
                model.provider.stream(wire, model.outbound),
            ) => stream
                .map_err(|_| BrainError::Transport("compactor response header timed out".into()))??,
            () = &mut total => return Err(BrainError::Transport("compactor call exceeded its total deadline".into())),
            () = model.cancel.cancelled() => return Err(BrainError::Cancelled),
        };
        let mut accumulator = Accumulator::new();
        loop {
            let idle = tokio::time::sleep(model.idle_timeout);
            tokio::pin!(idle);
            let event = tokio::select! {
                event = stream.next() => event,
                () = &mut idle => return Err(BrainError::Transport("compactor stream idle timeout".into())),
                () = &mut total => return Err(BrainError::Transport("compactor call exceeded its total deadline".into())),
                () = model.cancel.cancelled() => return Err(BrainError::Cancelled),
            };
            let Some(event) = event else { break };
            accumulator.push(event?)?;
        }
        if !accumulator.saw_terminal {
            return Err(BrainError::Protocol(
                "compactor stream ended without a terminal message event".into(),
            ));
        }
        let (message, stop, usage) = accumulator.finish()?;
        if stop != StopReason::EndTurn {
            return Err(BrainError::Protocol(format!(
                "compactor did not complete its summary (stop reason {stop:?})"
            )));
        }
        let mut summary = String::new();
        for block in message.content {
            match block {
                ContentBlock::Text { text } => summary.push_str(&text),
                ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. } => {
                    return Err(BrainError::Protocol(
                        "compactor returned a non-text block".into(),
                    ));
                }
            }
        }
        if summary.trim().is_empty() {
            return Err(BrainError::Protocol(
                "compactor returned an empty summary".into(),
            ));
        }
        Ok(CompactionResult {
            summary,
            provider: model.provider_name.to_owned(),
            model: model.session.prefix.model.clone(),
            usage,
        })
    }
}

fn build_compaction_wire(
    request: &CompactionRequest,
    model: &CompactionModel<'_>,
) -> Result<crate::provider::ModelRequest> {
    let mut definition = AgentDef::new(
        COMPACTION_INSTRUCTIONS,
        model.session.prefix.model.clone(),
        model.session.prefix.dialect,
    );
    definition.sampling = GenOpts {
        max_tokens: request.max_output_tokens,
        output_token_parameter: model.session.prefix.sampling.output_token_parameter,
        temperature: None,
        reasoning_effort: model.session.prefix.sampling.reasoning_effort.clone(),
        stop_sequences: Vec::new(),
    };
    let prefix = definition.seal();
    let payload = serde_json::json!({
        "previous_summary": &request.previous_summary,
        "new_canonical_span": &request.new_span,
    });
    let history = [Message::user_text(serde_jcs::to_string(&payload)?)];
    model.provider.build_request(
        &prefix,
        &history,
        &model.session.key,
        &model.session.base_url,
    )
}

#[derive(Debug, Clone)]
pub struct SemanticPlan {
    pub summary: String,
    pub summary_kind: String,
    pub compactor_provider: String,
    pub compactor_model: String,
    pub tail: Vec<Message>,
    pub retained_messages: u64,
    pub token_estimate: u64,
    pub source_context_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ContextPayload {
    pub summary: String,
    pub summary_kind: String,
    pub compactor_provider: String,
    pub compactor_model: String,
    pub tail: Vec<Message>,
}

#[derive(Debug, Clone)]
pub struct EncodedPayload {
    pub digest: String,
    pub chunks_base64: Vec<String>,
}

pub fn estimate_tokens(history: &[Message]) -> usize {
    history
        .iter()
        .map(|message| {
            4 + message
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => estimate_text_tokens(text),
                    ContentBlock::ToolUse { id, name, input } => {
                        8 + estimate_text_tokens(id)
                            + estimate_text_tokens(name)
                            + serde_json::to_vec(input).map_or(0, |bytes| bytes.len())
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => 8 + estimate_text_tokens(tool_use_id) + estimate_text_tokens(content),
                })
                .sum::<usize>()
        })
        .sum()
}

fn estimate_text_tokens(text: &str) -> usize {
    // One UTF-8 byte per possible token is the only provider-neutral upper bound available for
    // arbitrary compatible models. Less conservative ratios are unsafe for code, random ASCII,
    // byte-fallback encodings and tokenizers unknown to the neutral Brain runtime.
    text.len()
}

#[cfg(test)]
fn is_turn_opener(message: &Message) -> bool {
    message.role == crate::message::Role::User
        && message
            .content
            .iter()
            .all(|block| !matches!(block, ContentBlock::ToolResult { .. }))
}

pub fn plan(
    history: &[Message],
    has_installed_summary: bool,
    soft_tokens: usize,
    tail_tokens: usize,
    compaction_span_tokens: usize,
    force: bool,
) -> Option<CompactionPlan> {
    if !force && estimate_tokens(history) <= soft_tokens {
        return None;
    }
    let summary_offset = usize::from(has_installed_summary);
    if summary_offset >= history.len() {
        return None;
    }
    let span_budget = compaction_span_tokens.max(1_024);
    let source = &history[summary_offset..];
    let total_source_tokens = estimate_tokens(source);
    let mut raw_prefix_tokens = 0usize;
    let mut minimum_projected_tokens = 0usize;
    let mut pending = HashSet::new();
    let mut preferred_cuts = Vec::new();
    let mut fallback_cuts = Vec::new();
    for (relative_index, message) in source.iter().enumerate() {
        raw_prefix_tokens =
            raw_prefix_tokens.saturating_add(estimate_tokens(std::slice::from_ref(message)));
        minimum_projected_tokens = minimum_projected_tokens
            .saturating_add(minimum_compaction_tokens(std::slice::from_ref(message)));
        for block in &message.content {
            match block {
                ContentBlock::ToolUse { id, .. } => {
                    if !pending.insert(id.as_str()) {
                        return None;
                    }
                }
                ContentBlock::ToolResult { tool_use_id, .. } => {
                    if !pending.remove(tool_use_id.as_str()) {
                        return None;
                    }
                }
                ContentBlock::Text { .. } => {}
            }
        }
        if !pending.is_empty() || minimum_projected_tokens > span_budget {
            continue;
        }
        let cut = summary_offset + relative_index + 1;
        fallback_cuts.push(cut);
        if total_source_tokens.saturating_sub(raw_prefix_tokens) >= tail_tokens {
            preferred_cuts.push(cut);
        }
    }
    if !pending.is_empty() {
        return None;
    }
    let mut candidates = preferred_cuts;
    if candidates.is_empty() {
        candidates = fallback_cuts;
    }
    let (cut, new_span) = candidates.into_iter().rev().find_map(|cut| {
        project_compaction_span(&history[summary_offset..cut], span_budget).map(|span| (cut, span))
    })?;
    debug_assert!(closed_tool_pairs(&history[summary_offset..cut]));
    debug_assert!(closed_tool_pairs(&history[cut..]));
    let previous_summary = has_installed_summary.then(|| {
        history
            .first()
            .and_then(|message| {
                message.content.iter().find_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
            })
            .unwrap_or_default()
    });
    let tail = history[cut..].to_vec();
    Some(CompactionPlan {
        previous_summary,
        new_span,
        retained_messages: tail.len() as u64,
        tail,
        source_context_digest: digest_messages(history),
    })
}

fn closed_tool_pairs(messages: &[Message]) -> bool {
    let mut pending = HashSet::new();
    for message in messages {
        for block in &message.content {
            match block {
                ContentBlock::ToolUse { id, .. } => {
                    if !pending.insert(id.as_str()) {
                        return false;
                    }
                }
                ContentBlock::ToolResult { tool_use_id, .. } => {
                    if !pending.remove(tool_use_id.as_str()) {
                        return false;
                    }
                }
                ContentBlock::Text { .. } => {}
            }
        }
    }
    pending.is_empty()
}

/// Builds the bounded provider-visible compactor projection. Only ToolResult content may be
/// shortened; IDs, error flags, message roles and call/result grouping remain exact. The raw
/// journal and the installed retained tail are never modified. A digest/byte-count marker lets
/// the summarizer state honestly that the audit payload was larger than its context projection.
fn project_compaction_span(messages: &[Message], budget: usize) -> Option<Vec<Message>> {
    if estimate_tokens(messages) <= budget {
        return Some(messages.to_vec());
    }
    let result_count = messages
        .iter()
        .flat_map(|message| &message.content)
        .filter(|block| matches!(block, ContentBlock::ToolResult { .. }))
        .count();
    if result_count == 0 {
        return None;
    }

    let mut preview_bytes = COMPACTION_TOOL_RESULT_PREVIEW_BYTES.min(budget / result_count.max(1));
    loop {
        let projected = messages
            .iter()
            .cloned()
            .map(|mut message| {
                for block in &mut message.content {
                    let ContentBlock::ToolResult { content, .. } = block else {
                        continue;
                    };
                    if content.len() <= preview_bytes {
                        continue;
                    }
                    let original_bytes = content.len();
                    let digest = hex::encode(Sha256::digest(content.as_bytes()));
                    let preview = utf8_prefix(content, preview_bytes);
                    let projected = tool_result_projection(preview, original_bytes, &digest);
                    if projected.len() < content.len() {
                        *content = projected;
                    }
                }
                message
            })
            .collect::<Vec<_>>();
        if estimate_tokens(&projected) <= budget {
            return Some(projected);
        }
        if preview_bytes == 0 {
            return None;
        }
        preview_bytes /= 2;
    }
}

fn minimum_compaction_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|message| {
            4 + message
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Text { text } => estimate_text_tokens(text),
                    ContentBlock::ToolUse { id, name, input } => {
                        8 + id.len()
                            + name.len()
                            + serde_json::to_vec(input).map_or(0, |bytes| bytes.len())
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        let marker_len =
                            tool_result_projection("", content.len(), &"0".repeat(64)).len();
                        8 + tool_use_id.len() + content.len().min(marker_len)
                    }
                })
                .sum::<usize>()
        })
        .sum()
}

fn tool_result_projection(preview: &str, original_bytes: usize, digest: &str) -> String {
    format!(
        "{preview}\n[tool result compacted for summary; original_bytes={original_bytes}; sha256={digest}]"
    )
}

fn utf8_prefix(value: &str, max_bytes: usize) -> &str {
    let mut end = value.len().min(max_bytes);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub fn finish_plan(
    plan: CompactionPlan,
    result: CompactionResult,
    summary_token_budget: usize,
) -> Result<SemanticPlan> {
    if estimate_text_tokens(&result.summary) > summary_token_budget {
        return Err(BrainError::Protocol(format!(
            "compactor summary exceeds {summary_token_budget} estimated tokens"
        )));
    }
    Ok(SemanticPlan {
        token_estimate: estimate_text_tokens(&result.summary)
            .saturating_add(estimate_tokens(&plan.tail)) as u64,
        summary: result.summary,
        summary_kind: "semantic".into(),
        compactor_provider: result.provider,
        compactor_model: result.model,
        retained_messages: plan.retained_messages,
        tail: plan.tail,
        source_context_digest: plan.source_context_digest,
    })
}

fn digest_messages(messages: &[Message]) -> String {
    let bytes = serde_jcs::to_vec(messages).expect("messages serialize");
    hex::encode(Sha256::digest(bytes))
}

pub fn encode_payload(plan: &SemanticPlan) -> Result<EncodedPayload> {
    let bytes = serde_json::to_vec(&ContextPayload {
        summary: plan.summary.clone(),
        summary_kind: plan.summary_kind.clone(),
        compactor_provider: plan.compactor_provider.clone(),
        compactor_model: plan.compactor_model.clone(),
        tail: plan.tail.clone(),
    })?;
    let digest = hex::encode(Sha256::digest(&bytes));
    let chunks_base64 = bytes
        .chunks(CONTEXT_CHUNK_BYTES)
        .map(|chunk| base64::engine::general_purpose::STANDARD.encode(chunk))
        .collect();
    Ok(EncodedPayload {
        digest,
        chunks_base64,
    })
}

pub fn decode_payload(entries: &[Entry], pointer: &ContextPointerDoc) -> Result<ContextPayload> {
    let mut chunks: Vec<_> = entries
        .iter()
        .filter_map(|entry| match &entry.record {
            Record::ContextChunk {
                checkpoint_id,
                index,
                total,
                content_base64,
                ..
            } if checkpoint_id == &pointer.checkpoint_id => {
                Some((*index, *total, content_base64.as_str()))
            }
            _ => None,
        })
        .collect();
    chunks.sort_by_key(|(index, _, _)| *index);
    let total = chunks.first().map_or(0, |(_, total, _)| *total);
    if total == 0 || chunks.len() != total as usize {
        return Err(BrainError::Journal(format!(
            "context checkpoint {} is missing chunks",
            pointer.checkpoint_id
        )));
    }
    let mut bytes = Vec::new();
    for (expected, (index, chunk_total, encoded)) in chunks.into_iter().enumerate() {
        if index != expected as u32 || chunk_total != total {
            return Err(BrainError::Journal(
                "context checkpoint chunk order is invalid".into(),
            ));
        }
        bytes.extend(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| {
                    BrainError::Journal("context checkpoint chunk is not base64".into())
                })?,
        );
    }
    if hex::encode(Sha256::digest(&bytes)) != pointer.payload_digest {
        return Err(BrainError::Journal(
            "context checkpoint payload digest mismatch".into(),
        ));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn materialize_history(
    entries: &[Entry],
    pointer: Option<&ContextPointerDoc>,
) -> Result<Vec<Message>> {
    let (mut fold, after) = match pointer {
        Some(pointer) => {
            let payload = decode_payload(entries, pointer)?;
            let mut history = Vec::with_capacity(payload.tail.len() + 1);
            history.push(Message::user_text(payload.summary));
            history.extend(payload.tail);
            (
                crate::journal::Fold::from_history(history),
                pointer.covers_through_sequence,
            )
        }
        None => (crate::journal::Fold::default(), 0),
    };
    for entry in entries.iter().filter(|entry| entry.seq > after) {
        fold.apply(&entry.record);
    }
    fold.finish();
    Ok(fold.history)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(index: usize, bulk: usize) -> Vec<Message> {
        vec![
            Message::user_text(format!(
                "goal {index}: edit /workspace/item-{index}.rs {}",
                "x".repeat(bulk)
            )),
            Message::assistant(vec![ContentBlock::ToolUse {
                id: format!("call-{index}"),
                name: "bash".into(),
                input: serde_json::json!({"command": format!("test -f /workspace/item-{index}.rs")}),
            }]),
            Message::tool_results(vec![ContentBlock::ToolResult {
                tool_use_id: format!("call-{index}"),
                content: if index == 2 { "ERROR: missing" } else { "ok" }.into(),
                is_error: index == 2,
            }]),
            Message::assistant(vec![ContentBlock::text(format!(
                "decision {index}: continue"
            ))]),
        ]
    }

    #[test]
    fn a_small_declared_window_derives_bounded_normal_and_compactor_requests() {
        let rendered_base = serde_json::json!({
            "system": "继续处理这个多语言会话",
            "tools": [{"name":"lookup", "input_schema":{"type":"object","properties":{"键":{"type":"string"}}}}]
        });
        let budget = derive_context_budget(
            &rendered_base,
            32 * 1024,
            4 * 1024,
            DEFAULT_CONTEXT_SOFT_TOKENS,
            DEFAULT_CONTEXT_HARD_TOKENS,
            DEFAULT_CONTEXT_TAIL_TOKENS,
        )
        .unwrap();
        assert!(budget.soft_tokens < budget.hard_tokens);
        assert!(budget.hard_tokens < 32 * 1024 - 4 * 1024);
        assert_eq!(budget.summary_tokens, 4 * 1024);
        assert!(budget.tail_tokens <= budget.soft_tokens / 4);
        assert_eq!(estimate_text_tokens("继续处理"), "继续处理".len());
    }

    #[test]
    fn prefix_and_output_reserve_must_fit_before_any_provider_call() {
        let rendered_base = serde_json::json!({"tools":"x".repeat(40_000)});
        let error = derive_context_budget(
            &rendered_base,
            8 * 1024,
            4 * 1024,
            DEFAULT_CONTEXT_SOFT_TOKENS,
            DEFAULT_CONTEXT_HARD_TOKENS,
            DEFAULT_CONTEXT_TAIL_TOKENS,
        )
        .expect_err("oversized immutable prefix must reject create");
        assert!(matches!(error, BrainError::Invalid(_)));
    }

    #[test]
    fn provider_neutral_estimate_bounds_hostile_ascii_and_utf8_by_exact_bytes() {
        let ascii = (0..8_192)
            .map(|index| {
                const ALPHABET: &[u8] =
                    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
                ALPHABET[(index * 37 + 11) % ALPHABET.len()] as char
            })
            .collect::<String>();
        let emoji = "🧪🛰️🧠".repeat(1_024);
        assert_eq!(estimate_text_tokens(&ascii), ascii.len());
        assert_eq!(estimate_text_tokens(&emoji), emoji.len());
        assert!(estimate_text_tokens(&ascii) > ascii.len() / 4);
        assert!(estimate_text_tokens(&emoji) > emoji.chars().count());
    }

    #[test]
    fn one_huge_multi_round_turn_compacts_at_closed_pairs_with_bounded_wire() {
        let mut history = vec![Message::user_text("complete the long tool-driven task")];
        for index in 0..64 {
            history.push(Message::assistant(vec![ContentBlock::ToolUse {
                id: format!("call-{index}"),
                name: "inspect".into(),
                input: serde_json::json!({"path": format!("/workspace/{index}.txt")}),
            }]));
            history.push(Message::tool_results(vec![ContentBlock::ToolResult {
                tool_use_id: format!("call-{index}"),
                content: format!("round={index}\n{}", "x".repeat(94_000)),
                is_error: false,
            }]));
        }

        let original = history.clone();
        let original_digest = digest_messages(&history);
        let mut current = history;
        let mut reconstructed = Vec::new();
        let mut has_summary = false;
        let mut passes = 0usize;
        let first_plan = loop {
            if estimate_tokens(&current) <= 8_000 {
                break None;
            }
            let plan = plan(&current, has_summary, 8_000, 2_000, 4_000, false)
                .expect("a huge active turn must have a safe partial-turn handoff");
            assert!(closed_tool_pairs(&plan.new_span));
            assert!(closed_tool_pairs(&plan.tail));
            assert!(estimate_tokens(&plan.new_span) <= 4_000);
            assert!(plan.new_span.iter().any(|message| message.content.iter().any(
                |block| matches!(block, ContentBlock::ToolResult { content, .. } if content.contains("tool result compacted for summary"))
            )));
            let offset = usize::from(has_summary);
            let cut = current.len() - plan.tail.len();
            reconstructed.extend_from_slice(&current[offset..cut]);
            let captured = (passes == 0).then(|| plan.clone());
            current = vec![Message::user_text(format!("bounded handoff pass {passes}"))];
            current.extend(plan.tail);
            has_summary = true;
            passes += 1;
            assert!(passes <= 64, "bounded compaction must make progress");
            if captured.is_some() {
                break captured;
            }
        }
        .expect("first compaction plan");
        assert_eq!(first_plan.source_context_digest, original_digest);

        while estimate_tokens(&current) > 8_000 {
            let plan = plan(&current, true, 8_000, 2_000, 4_000, false)
                .expect("repeated compaction must keep making progress");
            let cut = current.len() - plan.tail.len();
            reconstructed.extend_from_slice(&current[1..cut]);
            current = vec![Message::user_text(format!("bounded handoff pass {passes}"))];
            current.extend(plan.tail);
            passes += 1;
            assert!(passes <= 64, "bounded compaction must make progress");
        }
        reconstructed.extend_from_slice(&current[1..]);
        assert_eq!(reconstructed, original);
        assert!(passes > 1);

        let compaction = CompactionRequest {
            previous_summary: first_plan.previous_summary.clone(),
            new_span: first_plan.new_span.clone(),
            max_output_tokens: 1_024,
        };
        let payload = serde_json::json!({
            "previous_summary": compaction.previous_summary,
            "new_canonical_span": compaction.new_span,
        });
        let compactor_history = [Message::user_text(serde_jcs::to_string(&payload).unwrap())];
        for provider in [
            Arc::new(crate::provider::openai::OpenAiChat) as Arc<dyn Provider>,
            Arc::new(crate::provider::anthropic::Anthropic) as Arc<dyn Provider>,
        ] {
            let mut definition = AgentDef::new(
                COMPACTION_INSTRUCTIONS,
                "compactor-model",
                provider.dialect(),
            );
            definition.sampling.max_tokens = 1_024;
            let prefix = definition.seal();
            let wire = provider
                .build_request(
                    &prefix,
                    &compactor_history,
                    &crate::config::ProviderKey::new("sentinel"),
                    "https://provider.example/v1",
                )
                .unwrap();
            serde_json::from_slice::<serde_json::Value>(&wire.body).unwrap();
            validate_model_request_budget(wire.body_len(), 1_024, 32 * 1_024, "test").unwrap();
        }
    }

    #[test]
    fn semantic_plan_preserves_tail_and_passes_only_the_new_span() {
        let history: Vec<_> = (0..20).flat_map(|index| turn(index, 512)).collect();
        let plan = plan(&history, false, 1_000, 400, 100_000, false).expect("over soft limit");
        assert!(is_turn_opener(&plan.tail[0]));
        assert!(plan.previous_summary.is_none());
        assert!(plan.new_span.iter().any(|message| {
            serde_json::to_string(message)
                .unwrap()
                .contains("/workspace/item-2.rs")
        }));
        for message in &plan.tail {
            for block in &message.content {
                if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                    assert!(plan.tail.iter().any(|candidate| {
                        candidate.tool_uses().any(|(id, _, _)| id == tool_use_id)
                    }));
                }
            }
        }
    }

    #[test]
    fn repeated_compaction_passes_the_full_prior_summary_and_keeps_tail_exact() {
        let history: Vec<_> = (0..30).flat_map(|index| turn(index, 4_096)).collect();
        let first = plan(&history, false, 2_000, 800, 100_000, false).unwrap();
        let retained = first.tail.clone();
        let first = finish_plan(
            first,
            CompactionResult {
                summary: "material decision id=decision-early path=/workspace/item-2.rs".into(),
                provider: "fake".into(),
                model: "compact-test".into(),
                usage: crate::message::Usage::default(),
            },
            SUMMARY_TOKEN_BUDGET,
        )
        .unwrap();
        let mut generation_two_history = vec![Message::user_text(first.summary.clone())];
        generation_two_history.extend(first.tail.clone());
        generation_two_history.extend((30..42).flat_map(|index| turn(index, 2_048)));
        let second_plan =
            plan(&generation_two_history, true, 2_000, 2_000, 100_000, false).unwrap();
        assert_eq!(
            second_plan.previous_summary.as_deref(),
            Some(first.summary.as_str())
        );
        assert!(
            !second_plan
                .new_span
                .iter()
                .any(|message| message == &generation_two_history[0])
        );
        let second = finish_plan(
            second_plan,
            CompactionResult {
                summary: format!("{}\nnew progress preserved", first.summary),
                provider: "fake".into(),
                model: "compact-test".into(),
                usage: crate::message::Usage::default(),
            },
            SUMMARY_TOKEN_BUDGET,
        )
        .unwrap();
        assert!(second.summary.contains("decision-early"));
        assert!(generation_two_history.ends_with(&second.tail));
        assert!(retained.iter().all(|message| history.contains(message)));
        let encoded = encode_payload(&second).unwrap();
        assert!(!encoded.chunks_base64.is_empty());
        assert!(
            encoded
                .chunks_base64
                .iter()
                .all(|chunk| chunk.len() < 96 * 1024)
        );
    }
}
