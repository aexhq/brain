//! Pure conversions between `contracts/agentloop/v1` shapes and kernel shapes, plus the
//! execution-boundary size validation the contract deliberately leaves out of schema.
//!
//! Everything here is data-in/data-out: no journal, no provider, no state. The effectful side
//! of every ctx op lives in `turn.rs`.

use brain_protocol::agentloop as al;
use serde_json::Value;

use crate::message::{ContentBlock, Message, Role, StopReason, Usage};
use crate::{BrainError, Result};

type OpError = al::AgentloopError;

fn invalid(message: impl Into<String>) -> OpError {
    crate::agentloop::op_error(al::AgentloopErrorCode::InvalidRequest, message, false)
}

pub(crate) fn identifier(value: &str) -> Result<al::Identifier> {
    value
        .parse()
        .map_err(|_| BrainError::Agentloop(format!("{value:?} is not a contract identifier")))
}

/// Sealed provider model names admit gateway-style path separators that plain contract
/// identifiers never do (e.g. "openai/gpt-4.1-nano").
pub(crate) fn model_name(value: &str) -> Result<al::ModelName> {
    value
        .parse()
        .map_err(|_| BrainError::Agentloop(format!("{value:?} is not a contract model name")))
}

pub(crate) fn seq(value: u64) -> Result<al::Seq> {
    Ok(al::Seq(std::num::NonZeroU64::new(value).ok_or_else(
        || BrainError::Agentloop("a journal seq of zero cannot cross the contract".into()),
    )?))
}

pub(crate) fn timestamp(at_ms: u64) -> al::Timestamp {
    al::Timestamp(
        chrono::DateTime::from_timestamp_millis(at_ms as i64).unwrap_or_else(chrono::Utc::now),
    )
}

/// RFC 8785 size of a JSON object, the canonical byte measure for every loop payload bound.
pub(crate) fn canonical_len(value: &serde_json::Map<String, Value>) -> Result<usize> {
    Ok(serde_jcs::to_vec(value)
        .map_err(|error| BrainError::Agentloop(format!("loop payload canonicalization: {error}")))?
        .len())
}

/// Validate one loop entry's data against its execution bound; `Ok` carries nothing, the
/// error is guest-visible.
pub(crate) fn validate_entry_data(
    data: &serde_json::Map<String, Value>,
    max_bytes: usize,
    kind: &str,
) -> std::result::Result<(), OpError> {
    let bytes = match canonical_len(data) {
        Ok(bytes) => bytes,
        Err(error) => return Err(invalid(error.to_string())),
    };
    if bytes > max_bytes {
        return Err(crate::agentloop::op_error(
            al::AgentloopErrorCode::EntryTooLarge,
            format!("{kind} entry data is {bytes} canonical bytes; the bound is {max_bytes}"),
            false,
        ));
    }
    Ok(())
}

/// Kernel content blocks to contract views. Admitted user messages and assistant messages only
/// ever hold text and tool-use blocks; anything else is a kernel invariant violation.
pub(crate) fn blocks_to_content_views(blocks: &[ContentBlock]) -> Result<Vec<al::ContentView>> {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => Ok(al::ContentView::TextContentView(text_view(text)?)),
            ContentBlock::ToolUse { id, name, input } => Ok(al::ContentView::ToolCallContentView(
                al::ToolCallContentView {
                    type_: al::ToolCallContentViewType::ToolCall,
                    tool_call_id: identifier(id)?,
                    name: identifier(name)?,
                    input: al::JsonObject(object_of(input)?),
                },
            )),
            ContentBlock::ToolResult { .. } => Err(BrainError::Agentloop(
                "a tool_result block cannot project into a message content view".into(),
            )),
        })
        .collect()
}

fn text_view(text: &str) -> Result<al::TextContentView> {
    Ok(al::TextContentView {
        type_: al::TextContentViewType::Text,
        // The contract caps one text block at 192 KiB; kernel text (bounded by record and
        // provider limits) fits, and a violation is an invariant failure, not user error.
        text: text.parse().map_err(|_| {
            BrainError::Agentloop("a text block exceeds the contract view bound".into())
        })?,
    })
}

fn object_of(value: &Value) -> Result<serde_json::Map<String, Value>> {
    value
        .as_object()
        .cloned()
        .ok_or_else(|| BrainError::Agentloop("a tool input is not a JSON object".into()))
}

/// Contract content views to kernel blocks (loop-composed request messages).
fn content_views_to_blocks(views: &[al::ContentView]) -> Vec<ContentBlock> {
    views
        .iter()
        .map(|view| match view {
            al::ContentView::TextContentView(text) => ContentBlock::Text {
                text: text.text.to_string(),
            },
            al::ContentView::ToolCallContentView(call) => ContentBlock::ToolUse {
                id: call.tool_call_id.to_string(),
                name: call.name.to_string(),
                input: Value::Object(call.input.0.clone()),
            },
        })
        .collect()
}

fn text_views_to_string(views: &[al::TextContentView]) -> String {
    views
        .iter()
        .map(|view| view.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A loop-composed message list to the kernel history shape the provider adapters render.
/// Consecutive tool_result messages fold into one user message, the Anthropic wire rule the
/// kernel history already follows.
pub(crate) fn model_messages_to_history(
    messages: &[al::ModelMessage],
) -> std::result::Result<Vec<Message>, OpError> {
    let mut history: Vec<Message> = Vec::with_capacity(messages.len());
    let mut pending_results: Vec<ContentBlock> = Vec::new();
    for message in messages {
        match message {
            al::ModelMessage::ToolResult {
                content,
                is_error,
                tool_call_id,
                ..
            } => pending_results.push(ContentBlock::ToolResult {
                tool_use_id: tool_call_id.to_string(),
                content: text_views_to_string(content),
                is_error: is_error.unwrap_or(false),
            }),
            al::ModelMessage::User { content } => {
                if !pending_results.is_empty() {
                    history.push(Message::tool_results(std::mem::take(&mut pending_results)));
                }
                history.push(Message {
                    role: Role::User,
                    content: content_views_to_blocks(content),
                });
            }
            al::ModelMessage::Assistant { content } => {
                if !pending_results.is_empty() {
                    history.push(Message::tool_results(std::mem::take(&mut pending_results)));
                }
                history.push(Message {
                    role: Role::Assistant,
                    content: content_views_to_blocks(content),
                });
            }
        }
    }
    if !pending_results.is_empty() {
        history.push(Message::tool_results(pending_results));
    }
    if history.is_empty() {
        return Err(invalid("a model request needs at least one message"));
    }
    Ok(history)
}

/// Project provider-history messages (a materialized context fork) into the contract's
/// ModelMessage shape — what a loop composes. Tool-result blocks recover their tool names
/// from the preceding tool_call blocks; an unmatched result keeps a neutral name.
pub(crate) fn history_to_model_messages(history: &[Message]) -> Result<Vec<al::ModelMessage>> {
    let mut names: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    let mut messages = Vec::with_capacity(history.len());
    for message in history {
        for block in &message.content {
            if let ContentBlock::ToolUse { id, name, .. } = block {
                names.insert(id.as_str(), name.as_str());
            }
        }
        match message.role {
            Role::Assistant => messages.push(al::ModelMessage::Assistant {
                content: blocks_to_content_views(&message.content)?,
            }),
            Role::User => {
                let mut plain = Vec::new();
                for block in &message.content {
                    match block {
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => messages.push(al::ModelMessage::ToolResult {
                            content: vec![text_view(content)?],
                            is_error: Some(*is_error),
                            name: identifier(
                                names.get(tool_use_id.as_str()).copied().unwrap_or("tool"),
                            )?,
                            tool_call_id: identifier(tool_use_id)?,
                        }),
                        other => plain.push(other.clone()),
                    }
                }
                if !plain.is_empty() {
                    messages.push(al::ModelMessage::User {
                        content: blocks_to_content_views(&plain)?,
                    });
                }
            }
        }
    }
    Ok(messages)
}

/// Validate a loop's tool presentations against the sealed grant and build the per-call tool
/// declarations. Presentation (description, schema, subset, order) is loop policy; the
/// executable route is copied from the seal and can never be widened here.
pub(crate) fn presented_tools(
    prefix: &crate::config::SealedPrefix,
    presentations: &[al::ToolPresentationView],
) -> std::result::Result<Vec<crate::config::ToolDecl>, OpError> {
    presentations
        .iter()
        .map(|presented| {
            let Some(sealed) = prefix.tool(presented.name.as_str()) else {
                return Err(crate::agentloop::op_error(
                    al::AgentloopErrorCode::UnsealedTool,
                    format!(
                        "tool {:?} is not in this session's sealed grant",
                        presented.name.as_str()
                    ),
                    false,
                ));
            };
            let mut decl = sealed.clone();
            decl.description = presented
                .description
                .as_ref()
                .map(|description| description.to_string())
                .unwrap_or_else(|| sealed.description.clone());
            decl.input_schema = Value::Object(presented.input_schema.0.clone());
            Ok(decl)
        })
        .collect()
}

fn stop_view(stop: StopReason) -> al::ModelStopReason {
    match stop {
        StopReason::ToolUse => al::ModelStopReason::ToolUse,
        StopReason::MaxTokens => al::ModelStopReason::MaxTokens,
        StopReason::Refusal => al::ModelStopReason::Refusal,
        // StopSequence and Unknown coarsen to end_turn in the view; the journal keeps truth.
        StopReason::EndTurn | StopReason::StopSequence | StopReason::Unknown => {
            al::ModelStopReason::EndTurn
        }
    }
}

fn usage_view(usage: &Usage) -> al::UsageView {
    al::UsageView {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_input_tokens,
        cache_write_tokens: usage.cache_creation_input_tokens,
        // The kernel never invents a total the provider did not report: absent is never zero.
        total_tokens: None,
    }
}

/// One dispatched result as the loop receives it, keyed by the loop's own call id.
pub(crate) fn tool_result_view(
    call: &al::ToolCallRequest,
    result: &crate::turn::DispatchedResultView,
) -> Result<al::ToolResultView> {
    Ok(al::ToolResultView {
        tool_call_id: call.tool_call_id.clone(),
        name: call.name.clone(),
        is_error: result.is_error,
        content: vec![text_view(&result.content)?],
    })
}

/// The complete folded round as the loop receives it.
pub(crate) fn assistant_view(
    message: &Message,
    stop: StopReason,
    usage: &Usage,
    model: &str,
) -> Result<al::AssistantMessageView> {
    Ok(al::AssistantMessageView {
        content: blocks_to_content_views(&message.content)?,
        model: model_name(model)?,
        stop_reason: stop_view(stop),
        usage: Some(usage_view(usage)),
    })
}

/// Project one journal record into its versioned typed view, or `None` for internal records.
/// `model` is the session's sealed model name — the model is sealed for the session's life,
/// so it is the producer of every assistant record.
pub(crate) fn project_entry(
    seq_value: u64,
    ts_ms: u64,
    model: &str,
    record: &crate::journal::Record,
) -> Result<Option<al::JournalEntryView>> {
    use crate::journal::Record;
    let at = timestamp(ts_ms);
    Ok(Some(match record {
        Record::UserMessage { content, .. } => al::JournalEntryView::UserMessage {
            at,
            content: blocks_to_content_views(content)?,
            seq: seq(seq_value)?,
        },
        Record::Assistant { content, stop, .. } => al::JournalEntryView::AssistantMessage {
            at,
            message: al::AssistantMessageView {
                content: blocks_to_content_views(content)?,
                model: model_name(model)?,
                stop_reason: stop_view(*stop),
                // Usage lives in its own record and is not re-joined into the projection.
                usage: None,
            },
            seq: seq(seq_value)?,
        },
        Record::ToolResult {
            call,
            name,
            content,
            is_error,
            ..
        } => al::JournalEntryView::ToolResult {
            at,
            result: al::ToolResultView {
                tool_call_id: identifier(call)?,
                name: identifier(name)?,
                is_error: *is_error,
                content: vec![text_view(content)?],
            },
            seq: seq(seq_value)?,
        },
        Record::LoopCustom { data, .. } => al::JournalEntryView::LoopCustom {
            at,
            data: al::JsonObject(data.clone()),
            seq: seq(seq_value)?,
        },
        Record::LoopEvent { name, data, .. } => al::JournalEntryView::LoopEvent {
            at,
            data: al::JsonObject(data.clone()),
            name: identifier(name)?,
            seq: seq(seq_value)?,
        },
        Record::LoopMark {
            covers_through_seq,
            data,
            ..
        } => al::JournalEntryView::LoopMark {
            at,
            covers_through_seq: seq(*covers_through_seq)?,
            data: al::JsonObject(data.clone()),
            seq: seq(seq_value)?,
        },
        _ => return Ok(None),
    }))
}

pub(crate) fn view_type(view: &al::JournalEntryView) -> al::CtxOpTypesItem {
    match view {
        al::JournalEntryView::UserMessage { .. } => al::CtxOpTypesItem::UserMessage,
        al::JournalEntryView::AssistantMessage { .. } => al::CtxOpTypesItem::AssistantMessage,
        al::JournalEntryView::ToolResult { .. } => al::CtxOpTypesItem::ToolResult,
        al::JournalEntryView::LoopCustom { .. } => al::CtxOpTypesItem::LoopCustom,
        al::JournalEntryView::LoopEvent { .. } => al::CtxOpTypesItem::LoopEvent,
        al::JournalEntryView::LoopMark { .. } => al::CtxOpTypesItem::LoopMark,
    }
}

pub(crate) fn view_seq(view: &al::JournalEntryView) -> u64 {
    match view {
        al::JournalEntryView::UserMessage { seq, .. }
        | al::JournalEntryView::AssistantMessage { seq, .. }
        | al::JournalEntryView::ToolResult { seq, .. }
        | al::JournalEntryView::LoopCustom { seq, .. }
        | al::JournalEntryView::LoopEvent { seq, .. }
        | al::JournalEntryView::LoopMark { seq, .. } => seq.0.get(),
    }
}

/// Classify a provider-round failure for the loop. Upstream failures are the loop's to handle
/// (retry, compact, fail the turn); kernel faults return as `Err` and always fail the turn.
pub(crate) fn provider_op_error(error: BrainError) -> Result<OpError> {
    let retryable = match &error {
        BrainError::Transport(_) | BrainError::Protocol(_) => true,
        BrainError::ProviderStatus { status, .. } => matches!(status, 408 | 429) || *status >= 500,
        BrainError::Cancelled => {
            return Ok(crate::agentloop::op_error(
                al::AgentloopErrorCode::Aborted,
                "the turn was cancelled",
                false,
            ));
        }
        // Anything else is kernel state, custody or journal trouble: never the loop's call.
        _ => return Err(error),
    };
    Ok(crate::agentloop::op_error(
        al::AgentloopErrorCode::ProviderError,
        error.to_string(),
        retryable,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(value: Value) -> serde_json::Map<String, Value> {
        value.as_object().cloned().expect("object")
    }

    #[test]
    fn tool_result_messages_fold_like_kernel_history() {
        let messages: Vec<al::ModelMessage> = serde_json::from_value(json!([
            {"role": "user", "content": [{"type": "text", "text": "hi"}]},
            {"role": "assistant", "content": [
                {"type": "text", "text": "calling"},
                {"type": "tool_call", "tool_call_id": "op_a", "name": "echo", "input": {}}
            ]},
            {"role": "tool_result", "tool_call_id": "op_a", "name": "echo",
             "content": [{"type": "text", "text": "pong"}]}
        ]))
        .expect("contract messages");
        let history = model_messages_to_history(&messages).expect("history");
        assert_eq!(history.len(), 3);
        assert_eq!(history[2].role, Role::User);
        assert!(matches!(
            &history[2].content[0],
            ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "op_a"
        ));
    }

    #[test]
    fn entry_bounds_use_canonical_bytes() {
        let small = object(json!({"note": "ok"}));
        assert!(validate_entry_data(&small, 64, "custom").is_ok());
        let big = object(json!({"note": "x".repeat(96)}));
        let error = validate_entry_data(&big, 64, "custom").expect_err("too large");
        assert_eq!(error.code, al::AgentloopErrorCode::EntryTooLarge);
    }

    #[test]
    fn loop_records_project_to_typed_views() {
        let view = project_entry(
            7,
            1_700_000_000_000,
            "claude-test",
            &crate::journal::Record::LoopEvent {
                turn: "trn".into(),
                name: "todo.updated".into(),
                data: json!({"open": 3}).as_object().cloned().expect("object"),
            },
        )
        .expect("projects")
        .expect("visible");
        assert_eq!(view_type(&view), al::CtxOpTypesItem::LoopEvent);
        assert_eq!(view_seq(&view), 7);
    }

    #[test]
    fn internal_records_do_not_project() {
        let none = project_entry(
            9,
            0,
            "claude-test",
            &crate::journal::Record::TurnStarted { turn: "trn".into() },
        )
        .expect("projects");
        assert!(none.is_none());
    }

    #[test]
    fn provider_failures_classify_for_the_loop() {
        let overload = provider_op_error(BrainError::ProviderStatus {
            status: 529,
            body: "overloaded".into(),
            retry_after_ms: None,
        })
        .expect("guest visible");
        assert_eq!(overload.code, al::AgentloopErrorCode::ProviderError);
        assert!(overload.retryable);
        let auth = provider_op_error(BrainError::ProviderStatus {
            status: 401,
            body: "bad key".into(),
            retry_after_ms: None,
        })
        .expect("guest visible");
        assert!(!auth.retryable);
        assert!(provider_op_error(BrainError::Fenced).is_err());
    }
}
