//! Session events: derivation from journal records, plus the live fan-out.
//!
//! The journal is the event log. A durable record derives at most one SSE event (so seq maps
//! 1:1); `assistant.delta` and `tool.output` are live-only -- they consume seqs from the same
//! counter but are never journaled, so a replay simply has gaps where the stream once was.
//! The complete `assistant.message` / `tool.result` events carry the durable content.

use crate::journal::Record;
use brain_protocol::session::{
    self, ApiError, ApiErrorCode, Event, EventStream, ProviderUsage, Timestamp, ToolOutcome,
};
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Bound on `tool.result.output_preview` in events; the journal record keeps the fuller
/// (still bounded) content the model saw.
pub const OUTPUT_PREVIEW_BYTES: usize = 4 * 1024;

pub fn ts(ms: u64) -> Timestamp {
    Timestamp(chrono::DateTime::from_timestamp_millis(ms as i64).unwrap_or_else(chrono::Utc::now))
}

fn seq_nz(seq: u64) -> NonZeroU64 {
    NonZeroU64::new(seq.max(1)).expect("nonzero")
}

fn preview(content: &str) -> (String, bool) {
    if content.len() <= OUTPUT_PREVIEW_BYTES {
        (content.to_string(), false)
    } else {
        let mut end = OUTPUT_PREVIEW_BYTES;
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        (content[..end].to_string(), true)
    }
}

/// Maps our neutral stop reasons onto the contract's turn-completed vocabulary.
pub fn stop_reason(s: &str) -> session::StopReason {
    match s {
        "end_turn" => session::StopReason::EndTurn,
        "refusal" => session::StopReason::Refusal,
        "max_rounds" => session::StopReason::MaxRounds,
        "cancelled" => session::StopReason::Cancelled,
        _ => session::StopReason::Error,
    }
}

fn tool_outcome(s: &str) -> ToolOutcome {
    match s {
        "completed" => ToolOutcome::Completed,
        "cancelled" => ToolOutcome::Cancelled,
        "deadline_exceeded" => ToolOutcome::DeadlineExceeded,
        "interrupted" => ToolOutcome::Interrupted,
        _ => ToolOutcome::Failed,
    }
}

fn provider_of(s: &str) -> session::Provider {
    match s {
        "openai" => session::Provider::Openai,
        "anthropic" => session::Provider::Anthropic,
        "deepseek" => session::Provider::Deepseek,
        "moonshot" => session::Provider::Moonshot,
        "xai" => session::Provider::Xai,
        _ => session::Provider::OpenaiCompatible,
    }
}

pub fn usage_of(u: &crate::message::Usage) -> ProviderUsage {
    ProviderUsage {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        cache_read_input_tokens: u.cache_read_input_tokens,
        cache_creation_input_tokens: u.cache_creation_input_tokens,
        reasoning_tokens: u.reasoning_tokens,
    }
}

/// Derives the SSE event for one durable record, or None for internal-only records.
/// `hand_info` supplies the `session.updated` payload (session facts live on HEAD, not in the
/// record, so the deriver is handed a snapshot).
pub fn derive(
    session_id: &str,
    seq: u64,
    ts_ms: u64,
    record: &Record,
    hand_info: &session::HandInfo,
) -> Option<Event> {
    let sid: session::SessionId = session_id.parse().ok()?;
    let at = ts(ts_ms);
    let seq = seq_nz(seq);
    let turn_of = |t: &str| t.parse::<session::TurnId>().ok();
    Some(match record {
        Record::UserMessage { .. } => return None,
        Record::TurnStarted { turn } => Event::TurnStarted {
            at,
            seq,
            session_id: sid,
            turn_id: turn_of(turn)?,
        },
        Record::Assistant {
            turn,
            agent,
            content,
            ..
        } => {
            let text: String = content
                .iter()
                .filter_map(|b| match b {
                    crate::message::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            Event::AssistantMessage {
                agent_id: agent.parse().ok()?,
                at,
                seq,
                session_id: sid,
                text,
                turn_id: turn_of(turn)?,
            }
        }
        Record::Usage {
            turn,
            agent,
            provider,
            model,
            usage,
        } => Event::ModelUsage {
            agent_id: agent.parse().ok()?,
            at,
            model: model.clone(),
            provider: provider_of(provider),
            seq,
            session_id: sid,
            turn_id: turn_of(turn)?,
            usage: usage_of(usage),
        },
        Record::ToolCall {
            turn,
            agent,
            call,
            name,
            input,
            detach,
        } => Event::ToolCall {
            agent_id: agent.parse().ok()?,
            at,
            call_id: call.parse().ok()?,
            detach: *detach,
            input: input.clone(),
            name: name.clone(),
            seq,
            session_id: sid,
            turn_id: turn_of(turn)?,
        },
        Record::ToolResult {
            turn,
            agent,
            call,
            name,
            outcome,
            content,
            is_error,
            exit_code,
            duration_ms,
            truncated,
        } => {
            let (p, cut) = preview(content);
            Event::ToolResult {
                agent_id: agent.parse().ok()?,
                at,
                call_id: call.parse().ok()?,
                duration_ms: *duration_ms,
                error: is_error.then(|| p.clone()),
                exit_code: *exit_code,
                name: name.clone(),
                outcome: tool_outcome(outcome),
                output_preview: p,
                seq,
                session_id: sid,
                truncated: *truncated || cut,
                turn_id: turn_of(turn)?,
            }
        }
        Record::TurnCompleted {
            turn,
            stop_reason: sr,
            rounds,
            tool_calls,
            result,
        } => Event::TurnCompleted {
            at,
            result: result.clone(),
            rounds: *rounds,
            seq,
            session_id: sid,
            stop_reason: stop_reason(sr),
            tool_calls: *tool_calls,
            turn_id: turn_of(turn)?,
        },
        Record::TurnFailed {
            turn,
            code,
            message,
            details,
        } => Event::TurnFailed {
            at,
            error: ApiError {
                code: error_code(code),
                details: details.clone(),
                message: message.clone(),
                param: None,
                request_id: None,
            },
            seq,
            session_id: sid,
            turn_id: turn_of(turn)?,
        },
        Record::State { state, turn } => Event::SessionUpdated {
            at,
            hand: hand_info.clone(),
            seq,
            session_id: sid,
            state: session_state(state),
            turn_id: turn.as_deref().and_then(|t| t.parse().ok()),
        },
        Record::HandLost {
            turn,
            interrupted,
            synced_ms,
        } => Event::HandLost {
            at,
            interrupted_calls: interrupted.iter().filter_map(|c| c.parse().ok()).collect(),
            seq,
            session_id: sid,
            turn_id: turn.as_deref().and_then(|t| t.parse().ok()),
            workspace_synced_at: synced_ms.map(ts),
        },
        Record::Compacted { .. } => return None,
    })
}

pub fn session_state(s: &str) -> session::SessionState {
    match s {
        "active" => session::SessionState::Active,
        "deleted" => session::SessionState::Deleted,
        "failed" => session::SessionState::Failed,
        _ => session::SessionState::Idle,
    }
}

pub fn error_code(s: &str) -> ApiErrorCode {
    s.parse()
        .unwrap_or_else(|_| "internal".parse().expect("static API error code"))
}

/// Live constructors for the two ephemeral event types.
pub fn delta_event(
    session_id: &str,
    seq: u64,
    turn: &str,
    agent: &str,
    text: String,
) -> Option<Event> {
    Some(Event::AssistantDelta {
        agent_id: agent.parse().ok()?,
        at: ts(crate::wall_ms()),
        seq: seq_nz(seq),
        session_id: session_id.parse().ok()?,
        text,
        turn_id: turn.parse().ok()?,
    })
}

pub fn output_event(
    session_id: &str,
    seq: u64,
    turn: &str,
    call: &str,
    stream: EventStream,
    offset: u64,
    text: String,
) -> Option<Event> {
    Some(Event::ToolOutput {
        at: ts(crate::wall_ms()),
        call_id: call.parse().ok()?,
        offset,
        seq: seq_nz(seq),
        session_id: session_id.parse().ok()?,
        stream,
        text,
        turn_id: turn.parse().ok()?,
    })
}

pub fn event_seq(e: &Event) -> u64 {
    match e {
        Event::TurnStarted { seq, .. }
        | Event::AssistantDelta { seq, .. }
        | Event::AssistantMessage { seq, .. }
        | Event::ToolCall { seq, .. }
        | Event::ToolOutput { seq, .. }
        | Event::ToolResult { seq, .. }
        | Event::AgentSpawned { seq, .. }
        | Event::AgentFinished { seq, .. }
        | Event::ModelUsage { seq, .. }
        | Event::SessionUpdated { seq, .. }
        | Event::HandLost { seq, .. }
        | Event::TurnCompleted { seq, .. }
        | Event::TurnFailed { seq, .. } => seq.get(),
    }
}

/// The wire `event:` name (the serde tag), extracted from the serialized form so the SSE
/// framing can never drift from the schema.
pub fn event_type(e: &Event) -> String {
    serde_json::to_value(e)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

/// Per-session live fan-out. Outlives the session actor (a follower with `follow=true` holds
/// a subscription across idle discard and rehydration), so it is keyed in a shared registry.
pub struct EventHub {
    channels: Mutex<HashMap<String, broadcast::Sender<Arc<Event>>>>,
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHub {
    pub fn new() -> Self {
        Self {
            channels: Mutex::new(HashMap::new()),
        }
    }

    fn sender(&self, session_id: &str) -> broadcast::Sender<Arc<Event>> {
        let mut map = self.channels.lock().expect("hub lock");
        map.entry(session_id.to_string())
            .or_insert_with(|| broadcast::channel(1024).0)
            .clone()
    }

    pub fn publish(&self, session_id: &str, event: Event) {
        // A send with no receivers is fine: the journal already has the durable ones.
        let _ = self.sender(session_id).send(Arc::new(event));
    }

    pub fn subscribe(&self, session_id: &str) -> broadcast::Receiver<Arc<Event>> {
        self.sender(session_id).subscribe()
    }

    pub fn drop_session(&self, session_id: &str) {
        self.channels.lock().expect("hub lock").remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ContentBlock;

    fn hand_info() -> session::HandInfo {
        session::HandInfo {
            generation: Some(1),
            last_sync_at: None,
            live_jobs: Some(0),
            shape: session::HandShape::X1gb,
            started_at: None,
            state: session::HandState::Ready,
            wall_deadline_at: None,
        }
    }

    #[test]
    fn user_message_and_compaction_derive_no_event() {
        let r = Record::UserMessage {
            turn: "trn_aaaaaaaaaaaaaaaaaaaa".into(),
            content: vec![],
            metadata: std::collections::HashMap::new(),
        };
        assert!(derive("ses_aaaaaaaaaaaaaaaaaaaa", 1, 0, &r, &hand_info()).is_none());
        let c = Record::Compacted {
            summary: "s".into(),
            kept: 0,
        };
        assert!(derive("ses_aaaaaaaaaaaaaaaaaaaa", 2, 0, &c, &hand_info()).is_none());
    }

    #[test]
    fn assistant_record_derives_the_concatenated_text() {
        let r = Record::Assistant {
            turn: "trn_aaaaaaaaaaaaaaaaaaaa".into(),
            agent: "root".into(),
            content: vec![
                ContentBlock::text("a"),
                ContentBlock::ToolUse {
                    id: "c".into(),
                    name: "bash".into(),
                    input: serde_json::json!({}),
                },
                ContentBlock::text("b"),
            ],
            stop: crate::message::StopReason::ToolUse,
        };
        let e = derive("ses_aaaaaaaaaaaaaaaaaaaa", 3, 7, &r, &hand_info()).unwrap();
        match e {
            Event::AssistantMessage { text, seq, .. } => {
                assert_eq!(text, "ab");
                assert_eq!(seq.get(), 3);
            }
            other => panic!("wrong event {other:?}"),
        }
    }

    #[test]
    fn long_tool_result_is_previewed_and_marked_truncated() {
        let r = Record::ToolResult {
            turn: "trn_aaaaaaaaaaaaaaaaaaaa".into(),
            agent: "root".into(),
            call: "op_1".into(),
            name: "bash".into(),
            outcome: "completed".into(),
            content: "x".repeat(OUTPUT_PREVIEW_BYTES + 100),
            is_error: false,
            exit_code: Some(0),
            duration_ms: 1,
            truncated: false,
        };
        match derive("ses_aaaaaaaaaaaaaaaaaaaa", 4, 0, &r, &hand_info()).unwrap() {
            Event::ToolResult {
                output_preview,
                truncated,
                error,
                ..
            } => {
                assert_eq!(output_preview.len(), OUTPUT_PREVIEW_BYTES);
                assert!(truncated);
                assert!(error.is_none(), "no error text on a completed call");
            }
            other => panic!("wrong event {other:?}"),
        }
    }

    #[test]
    fn event_type_matches_the_schema_tag() {
        let r = Record::TurnStarted {
            turn: "trn_aaaaaaaaaaaaaaaaaaaa".into(),
        };
        let e = derive("ses_aaaaaaaaaaaaaaaaaaaa", 1, 0, &r, &hand_info()).unwrap();
        assert_eq!(event_type(&e), "turn.started");
        assert_eq!(event_seq(&e), 1);
    }

    #[test]
    fn provider_refusal_remains_distinct_on_the_public_event() {
        assert_eq!(stop_reason("refusal"), session::StopReason::Refusal);
    }

    #[test]
    fn hub_delivers_to_a_subscriber_started_before_publish() {
        let hub = EventHub::new();
        let mut rx = hub.subscribe("ses_x");
        let r = Record::TurnStarted {
            turn: "trn_aaaaaaaaaaaaaaaaaaaa".into(),
        };
        let e = derive("ses_aaaaaaaaaaaaaaaaaaaa", 1, 0, &r, &hand_info()).unwrap();
        hub.publish("ses_x", e);
        let got = rx.try_recv().unwrap();
        assert_eq!(event_seq(&got), 1);
    }
}
