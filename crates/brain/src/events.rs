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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast};

/// Bound on `tool.result.output_preview` in events; the journal record keeps the fuller
/// (still bounded) content the model saw.
pub const OUTPUT_PREVIEW_BYTES: usize = 4 * 1024;
/// A slow follower is disconnected on lag and replays durable records from the journal. Keeping
/// this ring deliberately small bounds worst-case live fanout memory even when every event is at
/// the public 256-KiB ceiling.
pub const EVENT_HUB_RING_EVENTS: usize = 8;
pub const DEFAULT_MAX_EVENT_FOLLOWERS: usize = 64;

pub fn ts(ms: u64) -> Timestamp {
    // Timestamps are kernel-written wall-clock ms; a value chrono cannot represent is journal
    // corruption and must be loud — substituting a plausible time would mask it.
    Timestamp(
        chrono::DateTime::from_timestamp_millis(ms as i64).unwrap_or_else(|| {
            panic!("journal timestamp {ms}ms is outside the representable range")
        }),
    )
}

fn seq_nz(seq: u64) -> NonZeroU64 {
    // Journal seqs start at 1; a zero here is corruption, and clamping it to 1 would collide
    // with a real record's seq instead of failing.
    NonZeroU64::new(seq).unwrap_or_else(|| panic!("journal seq 0 is corrupt"))
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
///
/// This is a derived, non-authoritative projection: every `.ok()?` below is a deliberate
/// projection drop — a journal-written id that no longer parses as its contract type
/// suppresses that event from the stream while the journal keeps the truth (REST reads of
/// the same state error loudly instead; see `session_doc`).
pub fn derive(session_id: &str, seq: u64, ts_ms: u64, record: &Record) -> Option<Event> {
    let sid: session::SessionId = session_id.parse().ok()?;
    let at = ts(ts_ms);
    let seq = seq_nz(seq);
    let turn_of = |t: &str| t.parse::<session::TurnId>().ok();
    Some(match record {
        Record::UserMessage {
            turn,
            starts_turn: true,
            ..
        } => Event::TurnStarted {
            at,
            seq,
            session_id: sid,
            turn_id: turn_of(turn)?,
        },
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
            attempt_id,
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
                attempt_id: attempt_id.parse().ok()?,
                seq,
                session_id: sid,
                text,
                turn_id: turn_of(turn)?,
            }
        }
        Record::ModelAttemptSuperseded {
            turn,
            logical_operation_id,
            superseded_attempt_id,
            replacement_attempt_id,
            reason,
        } => Event::ModelAttemptSuperseded {
            at,
            logical_operation_id: logical_operation_id.parse().ok()?,
            reason: reason.parse().ok()?,
            replacement_attempt_id: replacement_attempt_id.parse().ok()?,
            seq,
            session_id: sid,
            superseded_attempt_id: superseded_attempt_id.parse().ok()?,
            turn_id: turn_of(turn)?,
        },
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
            seq,
            session_id: sid,
            state: session_state(*state),
            turn_phase: None,
            turn_id: turn.as_deref().and_then(|t| t.parse().ok()),
            turn_state: session_turn_state(turn.as_deref()),
        },
        Record::HandLost {
            turn,
            interrupted,
            synced_ms: _,
        } => Event::HandLost {
            at,
            interrupted_calls: interrupted.iter().filter_map(|c| c.parse().ok()).collect(),
            seq,
            session_id: sid,
            turn_id: turn.as_deref().and_then(|t| t.parse().ok()),
        },
        Record::StorageUploadReserved {
            published_bytes,
            reserved_bytes,
            ..
        }
        | Record::StorageUploadPublished {
            published_bytes,
            reserved_bytes,
            ..
        }
        | Record::StorageUploadCompleted {
            published_bytes,
            reserved_bytes,
            ..
        }
        | Record::StorageUploadExpired {
            published_bytes,
            reserved_bytes,
            ..
        }
        | Record::StorageDeleteIntent {
            published_bytes,
            reserved_bytes,
            ..
        }
        | Record::StorageDeleteCompleted {
            published_bytes,
            reserved_bytes,
            ..
        } => Event::StorageUsage {
            at,
            seq,
            session_id: sid,
            storage: session::StorageInfo {
                session_storage_bytes: *published_bytes,
                upload_reserved_bytes: *reserved_bytes,
            },
        },
        Record::LoopEvent { turn, name, data } => Event::LoopEvent {
            at,
            data: data.clone(),
            name: name.parse().ok()?,
            seq,
            session_id: sid,
            turn_id: turn_of(turn)?,
        },
        Record::ContextChunk { .. }
        | Record::ContextInstalled { .. }
        | Record::DefaultSandboxChanged { .. }
        | Record::ModelCallIntent { .. }
        | Record::ModelCallUnknown { .. }
        | Record::ModelCallCompleted { .. }
        | Record::CompactionIntent { .. }
        | Record::CompactionUnknown { .. }
        | Record::CompactionCompleted { .. }
        | Record::CustomerCallIntent { .. }
        | Record::ManagedCallIntent { .. }
        | Record::ManagedCallAccepted { .. }
        | Record::ManagedCallUnknown { .. }
        | Record::CustomerTerminalReceived { .. }
        | Record::CustomerTerminalAcknowledged { .. }
        | Record::ManagedTerminalReceived { .. }
        | Record::ManagedTerminalAcknowledged { .. }
        | Record::SandboxFileEffectIntent { .. }
        | Record::SandboxFileEffectCompleted { .. }
        // Loop custom entries, marks and kv are durable loop state, not application events;
        // only the `event` entry kind is application-visible.
        | Record::LoopCustom { .. }
        | Record::LoopMark { .. }
        | Record::LoopKvSet { .. } => return None,
    })
}

pub fn session_state(s: crate::journal::SessionLifecycle) -> session::SessionState {
    use crate::journal::SessionLifecycle;
    match s {
        SessionLifecycle::Open => session::SessionState::Open,
        SessionLifecycle::Ending => session::SessionState::Ending,
        SessionLifecycle::Ended => session::SessionState::Ended,
        SessionLifecycle::Deleting => session::SessionState::Deleting,
        SessionLifecycle::Deleted => session::SessionState::Deleted,
        SessionLifecycle::Failed => session::SessionState::Failed,
    }
}

pub fn session_turn_state(turn: Option<&str>) -> session::SessionTurnState {
    if turn.is_some() {
        session::SessionTurnState::Running
    } else {
        session::SessionTurnState::Idle
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
    attempt_id: &str,
    text: String,
) -> Option<Event> {
    Some(Event::AssistantDelta {
        agent_id: agent.parse().ok()?,
        at: ts(crate::wall_ms()),
        attempt_id: attempt_id.parse().ok()?,
        provisional: true,
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
        Event::ReplayComplete { through_seq, .. } => *through_seq,
        Event::TurnStarted { seq, .. }
        | Event::AssistantDelta { seq, .. }
        | Event::AssistantMessage { seq, .. }
        | Event::ModelAttemptSuperseded { seq, .. }
        | Event::ToolCall { seq, .. }
        | Event::ToolOutput { seq, .. }
        | Event::ToolResult { seq, .. }
        | Event::AgentSpawned { seq, .. }
        | Event::AgentFinished { seq, .. }
        | Event::ModelUsage { seq, .. }
        | Event::StorageUsage { seq, .. }
        | Event::SessionUpdated { seq, .. }
        | Event::HandLost { seq, .. }
        | Event::TurnCompleted { seq, .. }
        | Event::TurnFailed { seq, .. }
        | Event::LoopEvent { seq, .. } => seq.get(),
    }
}

/// Live previews are deliberately outside the durable SSE cursor. They may carry a sequence as
/// ordering metadata, but reconnect must resume from the last journal-derived event so a crash
/// cannot make `Last-Event-ID` skip the UNKNOWN/supersession record that follows partial bytes.
pub fn event_is_ephemeral(event: &Event) -> bool {
    matches!(
        event,
        Event::AssistantDelta { .. } | Event::ToolOutput { .. } | Event::ReplayComplete { .. }
    )
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
    channels: Mutex<HashMap<String, Arc<HubChannel>>>,
    follower_permits: Arc<Semaphore>,
}

struct HubChannel {
    sender: broadcast::Sender<Arc<Event>>,
    subscribers: AtomicUsize,
}

/// One ref-counted live subscription. Dropping the last subscription removes the registry
/// entry; publishing to an idle session never creates one.
pub struct EventSubscription {
    hub: std::sync::Weak<EventHub>,
    session_id: String,
    channel: Arc<HubChannel>,
    receiver: broadcast::Receiver<Arc<Event>>,
    _permit: OwnedSemaphorePermit,
}

impl EventSubscription {
    pub async fn recv(&mut self) -> std::result::Result<Arc<Event>, broadcast::error::RecvError> {
        self.receiver.recv().await
    }

    #[cfg(test)]
    fn try_recv(&mut self) -> std::result::Result<Arc<Event>, broadcast::error::TryRecvError> {
        self.receiver.try_recv()
    }
}

impl Drop for EventSubscription {
    fn drop(&mut self) {
        let Some(hub) = self.hub.upgrade() else {
            return;
        };
        let previous = self.channel.subscribers.fetch_sub(1, Ordering::AcqRel);
        if previous != 1 {
            return;
        }
        let mut channels = hub.channels.lock().expect("hub lock");
        let is_current = channels
            .get(&self.session_id)
            .is_some_and(|current| Arc::ptr_eq(current, &self.channel));
        if is_current && self.channel.subscribers.load(Ordering::Acquire) == 0 {
            channels.remove(&self.session_id);
        }
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHub {
    pub fn new() -> Self {
        Self::with_max_followers(DEFAULT_MAX_EVENT_FOLLOWERS)
    }

    pub fn with_max_followers(max_followers: usize) -> Self {
        assert!(max_followers > 0, "event follower limit must be positive");
        Self {
            channels: Mutex::new(HashMap::new()),
            follower_permits: Arc::new(Semaphore::new(max_followers)),
        }
    }

    fn channel(&self, session_id: &str) -> Arc<HubChannel> {
        let mut map = self.channels.lock().expect("hub lock");
        map.entry(session_id.to_string())
            .or_insert_with(|| {
                Arc::new(HubChannel {
                    sender: broadcast::channel(EVENT_HUB_RING_EVENTS).0,
                    subscribers: AtomicUsize::new(0),
                })
            })
            .clone()
    }

    pub fn publish(&self, session_id: &str, event: Event) {
        // Durable events are replayable from the journal. If nobody follows this session now,
        // do not retain a broadcast channel merely because work occurred.
        let channel = self
            .channels
            .lock()
            .expect("hub lock")
            .get(session_id)
            .cloned();
        if let Some(channel) = channel {
            let _ = channel.sender.send(Arc::new(event));
        }
    }

    pub fn subscribe(
        self: &Arc<Self>,
        session_id: &str,
    ) -> std::result::Result<EventSubscription, tokio::sync::TryAcquireError> {
        // Admit before creating the per-session ring. Saturated callers receive deterministic
        // backpressure and cannot force an otherwise-unused channel into the registry.
        let permit = self.follower_permits.clone().try_acquire_owned()?;
        let channel = self.channel(session_id);
        channel.subscribers.fetch_add(1, Ordering::AcqRel);
        Ok(EventSubscription {
            hub: Arc::downgrade(self),
            session_id: session_id.to_string(),
            receiver: channel.sender.subscribe(),
            channel,
            _permit: permit,
        })
    }

    pub fn drop_session(&self, session_id: &str) {
        self.channels.lock().expect("hub lock").remove(session_id);
    }

    #[cfg(test)]
    fn channel_count(&self) -> usize {
        self.channels.lock().expect("hub lock").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::ContentBlock;

    #[test]
    fn user_message_and_compaction_derive_no_event() {
        let r = Record::UserMessage {
            turn: "trn_aaaaaaaaaaaaaaaaaaaa".into(),
            content: vec![],
            starts_turn: false,
            metadata: std::collections::HashMap::new(),
            idempotency_key_hash: None,
            request_hash: None,
        };
        assert!(derive("ses_aaaaaaaaaaaaaaaaaaaa", 1, 0, &r).is_none());
        let c = Record::ContextInstalled {
            checkpoint_id: "ctx_1".into(),
            base_checkpoint_id: None,
            covers_through_sequence: 1,
            retained_messages: 0,
            payload_digest: "a".repeat(64),
            base_prefix_digest: "b".repeat(64),
            source_context_digest: "c".repeat(64),
            token_estimate: 1,
            context_generation: 1,
            summary_kind: "semantic".into(),
            compactor_provider: "fake".into(),
            compactor_model: "fake".into(),
            retained_from_sequence: 1,
            created_at_ms: 0,
        };
        assert!(derive("ses_aaaaaaaaaaaaaaaaaaaa", 2, 0, &c).is_none());
    }

    #[test]
    fn assistant_record_derives_the_concatenated_text() {
        let r = Record::Assistant {
            turn: "trn_aaaaaaaaaaaaaaaaaaaa".into(),
            agent: "root".into(),
            attempt_id: "att_aaaaaaaaaaaaaaaaaaaa".into(),
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
        let e = derive("ses_aaaaaaaaaaaaaaaaaaaa", 3, 7, &r).unwrap();
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
        match derive("ses_aaaaaaaaaaaaaaaaaaaa", 4, 0, &r).unwrap() {
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
        let e = derive("ses_aaaaaaaaaaaaaaaaaaaa", 1, 0, &r).unwrap();
        assert_eq!(event_type(&e), "turn.started");
        assert_eq!(event_seq(&e), 1);
    }

    #[test]
    fn storage_transition_derives_exact_post_transition_gauges() {
        let record = Record::StorageUploadReserved {
            transfer_id: "xfer_aaaaaaaaaaaaaaaaaaaa".into(),
            key: "objects/data.bin".into(),
            bytes: 7,
            sha256: Some("a".repeat(64)),
            expires_at_ms: 99,
            published_bytes: 41,
            reserved_bytes: 7,
        };
        let event = derive("ses_aaaaaaaaaaaaaaaaaaaa", 8, 1_700_000_000_000, &record)
            .expect("storage transition is public");
        assert_eq!(event_type(&event), "storage.usage");
        assert_eq!(event_seq(&event), 8);
        let json = serde_json::to_value(event).expect("event serializes");
        assert_eq!(json["storage"]["session_storage_bytes"], 41);
        assert_eq!(json["storage"]["upload_reserved_bytes"], 7);
        assert!(json["storage"].get("sandbox_suspended_bytes").is_none());
        assert_eq!(json["at"], "2023-11-14T22:13:20Z");
    }

    #[test]
    fn provider_refusal_remains_distinct_on_the_public_event() {
        assert_eq!(stop_reason("refusal"), session::StopReason::Refusal);
    }

    #[test]
    fn lifecycle_and_turn_activity_are_independent_on_session_updates() {
        let cases = [
            (
                crate::journal::SessionLifecycle::Open,
                session::SessionState::Open,
                session::SessionTurnState::Idle,
            ),
            (
                crate::journal::SessionLifecycle::Ending,
                session::SessionState::Ending,
                session::SessionTurnState::Idle,
            ),
            (
                crate::journal::SessionLifecycle::Ended,
                session::SessionState::Ended,
                session::SessionTurnState::Idle,
            ),
            (
                crate::journal::SessionLifecycle::Deleting,
                session::SessionState::Deleting,
                session::SessionTurnState::Idle,
            ),
            (
                crate::journal::SessionLifecycle::Deleted,
                session::SessionState::Deleted,
                session::SessionTurnState::Idle,
            ),
            (
                crate::journal::SessionLifecycle::Failed,
                session::SessionState::Failed,
                session::SessionTurnState::Idle,
            ),
        ];

        for (internal, lifecycle, turn_state) in cases {
            assert_eq!(session_state(internal), lifecycle);
            assert_eq!(session_turn_state(None), turn_state);
            let event = derive(
                "ses_aaaaaaaaaaaaaaaaaaaa",
                1,
                0,
                &Record::State {
                    state: internal,
                    turn: None,
                },
            )
            .expect("known lifecycle derives an event");
            match event {
                Event::SessionUpdated {
                    state,
                    turn_state: actual_turn_state,
                    ..
                } => {
                    assert_eq!(state, lifecycle);
                    assert_eq!(actual_turn_state, turn_state);
                }
                other => panic!("wrong event {other:?}"),
            }
        }

        assert_eq!(
            session_turn_state(Some("trn_aaaaaaaaaaaaaaaaaaaa")),
            session::SessionTurnState::Running
        );
    }

    #[test]
    fn hub_delivers_to_a_subscriber_started_before_publish() {
        let hub = Arc::new(EventHub::new());
        let mut rx = hub.subscribe("ses_x").unwrap();
        let r = Record::TurnStarted {
            turn: "trn_aaaaaaaaaaaaaaaaaaaa".into(),
        };
        let e = derive("ses_aaaaaaaaaaaaaaaaaaaa", 1, 0, &r).unwrap();
        hub.publish("ses_x", e);
        let got = rx.try_recv().unwrap();
        assert_eq!(event_seq(&got), 1);
        drop(rx);
        assert_eq!(hub.channel_count(), 0);
    }

    #[test]
    fn publishing_without_subscribers_retains_no_channel() {
        let hub = EventHub::new();
        let r = Record::TurnStarted {
            turn: "trn_aaaaaaaaaaaaaaaaaaaa".into(),
        };
        let e = derive("ses_aaaaaaaaaaaaaaaaaaaa", 1, 0, &r).unwrap();
        for index in 0..10_000 {
            hub.publish(&format!("ses_{index:020}"), e.clone());
        }
        assert_eq!(hub.channel_count(), 0);
    }

    #[test]
    fn follower_admission_is_process_bounded_and_recovers_on_drop() {
        let hub = Arc::new(EventHub::with_max_followers(2));
        let first = hub.subscribe("ses_1").unwrap();
        let second = hub.subscribe("ses_2").unwrap();
        assert!(hub.subscribe("ses_3").is_err());
        assert_eq!(hub.channel_count(), 2);

        drop(first);
        let third = hub.subscribe("ses_3").unwrap();
        assert_eq!(hub.channel_count(), 2);
        drop(second);
        drop(third);
        assert_eq!(hub.channel_count(), 0);
    }

    #[test]
    fn slow_follower_lags_after_the_small_fixed_ring() {
        let hub = Arc::new(EventHub::with_max_followers(1));
        let mut follower = hub.subscribe("ses_x").unwrap();
        for seq in 1..=(EVENT_HUB_RING_EVENTS as u64 + 1) {
            let event = derive(
                "ses_aaaaaaaaaaaaaaaaaaaa",
                seq,
                0,
                &Record::TurnStarted {
                    turn: "trn_aaaaaaaaaaaaaaaaaaaa".into(),
                },
            )
            .unwrap();
            hub.publish("ses_x", event);
        }
        assert!(matches!(
            follower.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(1))
        ));
    }
}
