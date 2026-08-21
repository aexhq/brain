//! An in-process fake provider.
//!
//! Purpose is the same as Pi's `fauxProvider` and `oc-fake.mjs`: drive the loop
//! with **no credentials, no network and no token spend**, and with a
//! deterministic load shape so latency and memory numbers are reproducible.
//!
//! It carries the same guard Pi's does, for the same reason. Pi's faux provider
//! does **not** throw on queue exhaustion -- it emits an ordinary assistant turn
//! whose text is "No more faux responses queued" and the prompt resolves
//! successfully, so a density run silently becomes a measurement of how fast the
//! runtime produces error turns. `assert_drained` is not optional.

use super::{ModelRequest, Provider, ProviderEvent};
use crate::config::{Dialect, ProviderKey, SealedPrefix};
use crate::message::{Message, StopReason, Usage};
use crate::{BrainError, Result};
use futures_util::stream::BoxStream;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// One scripted assistant turn.
#[derive(Debug, Clone)]
pub enum Scripted {
    Text(String),
    Refusal(String),
    /// Ambiguous transport loss, optionally after a visible streamed prefix.
    TransportError {
        partial_text: Option<String>,
        message: String,
    },
    /// A complete HTTP error response. 4xx is deterministic; 5xx uses the unknown budget.
    ProviderStatus {
        status: u16,
        body: String,
    },
    /// N tool calls **in one assistant message** -- this sameness is what makes
    /// them parallel rather than N turns.
    ToolCalls(Vec<(String, String, serde_json::Value)>),
}

impl Scripted {
    pub fn tool(name: &str, input: serde_json::Value) -> Self {
        Scripted::ToolCalls(vec![(format!("call_{name}"), name.to_string(), input)])
    }
    pub fn parallel(n: usize, name: &str) -> Self {
        Scripted::ToolCalls(
            (0..n)
                .map(|i| {
                    (
                        format!("call_{i}"),
                        name.to_string(),
                        serde_json::json!({ "i": i }),
                    )
                })
                .collect(),
        )
    }
}

/// How the fake decides what to answer.
#[derive(Debug, Clone)]
pub enum FakeMode {
    /// A FIFO of scripted turns. Deterministic, but **only safe for one session
    /// at a time**: with K sessions in flight, session A pops session B's turn.
    /// The concurrency arm's guard caught exactly this, which is why `Policy`
    /// exists.
    Queue,
    /// Stateless. The answer is a pure function of the request, so any number of
    /// sessions can be in flight and each gets the turn its own history implies.
    /// This is the shape `oc-fake.mjs` uses, for the same reason.
    Policy {
        /// Emit tool calls until the request already contains this many
        /// assistant messages, then emit text.
        tool_rounds: u32,
        /// Tool calls per assistant message. p90 batch size is 4.
        parallel: usize,
        tool: String,
        /// Bytes of text in the final turn. With `tokens_per_second > 0` this is
        /// what gives a turn a realistic duration.
        text_bytes: usize,
    },
}

#[derive(Debug)]
pub struct FakeProvider {
    dialect: Dialect,
    pub mode: Mutex<FakeMode>,
    queue: Mutex<std::collections::VecDeque<Scripted>>,
    pub call_count: AtomicU64,
    /// Every request body that arrived, for the wire cross-check. This is the
    /// instrument, not a debug aid: it is how "the request the loop actually
    /// built" is verified against the request we think it built.
    pub arrivals: Mutex<Vec<Arrival>>,
    /// Emitted-per-token pacing, in tokens/second. 0 = as fast as possible.
    /// Non-zero is what gives a turn a realistic duration, which is what makes
    /// scheduler-lag and tail-latency numbers reproducible rather than
    /// measurements of an empty loop.
    pub tokens_per_second: AtomicU64,
    /// Inspect (fully parse + canonically hash + record) 1 request in every N.
    /// `1` inspects every request and is the default.
    ///
    /// Unpaced benchmarks need this because they drive the provider at the
    /// platform's real context size: a full `serde_json` parse of a ~500 KB body
    /// on every model call costs several milliseconds, which is more than the
    /// entire brain loop it is supposed to be measuring. An instrument that
    /// dominates its measurement is not an instrument.
    pub inspect_every: AtomicU64,
    /// Cap on retained arrivals. Unbounded retention is a leak inside the thing
    /// being measured and becomes significant at benchmark throughput.
    pub arrivals_cap: AtomicU64,
    /// Requests whose cheap byte-scan round count disagreed with the full parse
    /// on an inspected request. **Must be zero**; the harness treats any
    /// non-zero value as a fired guard, because a fake that mis-reads the round
    /// serves the wrong turn and every downstream count is wrong with it.
    pub policy_scan_mismatches: AtomicU64,
}

#[derive(Debug, Clone)]
pub struct Arrival {
    pub at_ns: u128,
    pub url: String,
    pub body_bytes: usize,
    pub message_count: usize,
    pub tools_offered: usize,
    pub system_chars: usize,
    /// SHA-256 over the canonical `{model, system, messages, tools[{name,
    /// description, input_schema}]}`. Cross-runtime request equivalence.
    pub context_sha256: String,
    /// The same over prefix-only content, so a prefix change is separable from
    /// a history change.
    pub prefix_sha256: String,
}

impl FakeProvider {
    pub fn new(dialect: Dialect) -> Self {
        FakeProvider {
            dialect,
            mode: Mutex::new(FakeMode::Queue),
            queue: Mutex::new(Default::default()),
            call_count: AtomicU64::new(0),
            arrivals: Mutex::new(Vec::new()),
            tokens_per_second: AtomicU64::new(0),
            inspect_every: AtomicU64::new(1),
            arrivals_cap: AtomicU64::new(u64::MAX),
            policy_scan_mismatches: AtomicU64::new(0),
        }
    }

    pub fn set_mode(&self, m: FakeMode) {
        *self.mode.lock().expect("fake mode") = m;
    }

    /// Assistant messages in the CURRENT turn, read from the raw request bytes
    /// without parsing them.
    ///
    /// The full-parse version of this (below) costs several milliseconds on a
    /// 122 K-token request, which is more than the Brain loop this benchmark is trying to
    /// measure. This byte scan is microseconds, and still derived purely from
    /// the request, so K concurrent sessions cannot interfere.
    ///
    /// A turn boundary is a REAL user message. On the Anthropic wire, tool
    /// results also ride in `role:"user"` messages (as `tool_result` blocks) —
    /// treating those as boundaries resets the round count every tool round and
    /// the policy emits tool calls until the round cap (the benchmark once observed
    /// exactly that: 2,560 model calls where 60 were expected).
    ///
    /// It is a heuristic on bytes, so it is **cross-checked against the full
    /// parse on every inspected request** and any disagreement increments
    /// `policy_scan_mismatches`, which the harness treats as a fired guard. A
    /// fake that mis-reads the round serves the wrong turn, and every count
    /// downstream is then wrong in a way that looks entirely plausible.
    fn assistants_this_turn_scan(body: &[u8]) -> u32 {
        const ROLE: &[u8] = b"\"role\":\"";
        const TOOL_RESULT: &[u8] = b"\"tool_result\"";
        // One forward pass: every role key, classified by the byte after the quote. `u` = user,
        // `a` = assistant. A message's content may serialize BEFORE its role key (serde_json
        // sorts keys) or after (declared order), so the window that surely contains message
        // i's own content — and no tool-result content from a sibling that matters — is
        // (end of role i-1, start of role i+1). A tool result always sits between two
        // assistant messages (or ends the list), so a REAL user's window never contains
        // another message's `tool_result` block in either layout.
        let mut roles: Vec<(usize, u8)> = Vec::new();
        let mut at = 0;
        while let Some(j) = find_from(&body[at..], ROLE) {
            let pos = at + j;
            let kind = body.get(pos + ROLE.len()).copied().unwrap_or(0);
            roles.push((pos, kind));
            at = pos + ROLE.len();
        }
        let mut anchor = 0;
        for i in (0..roles.len()).rev() {
            if roles[i].1 != b'u' {
                continue;
            }
            let win_start = if i > 0 {
                roles[i - 1].0 + ROLE.len()
            } else {
                0
            };
            let win_end = roles.get(i + 1).map_or(body.len(), |r| r.0);
            if find_from(&body[win_start..win_end], TOOL_RESULT).is_none() {
                anchor = roles[i].0;
                break;
            }
        }
        roles
            .iter()
            .filter(|(pos, kind)| *pos > anchor && *kind == b'a')
            .count() as u32
    }

    /// The reference: the same quantity, from a parsed body.
    fn assistants_this_turn_parsed(body: &serde_json::Value) -> u32 {
        let Some(a) = body.get("messages").and_then(|v| v.as_array()) else {
            return 0;
        };
        let is_real_user = |m: &serde_json::Value| {
            m.get("role").and_then(|r| r.as_str()) == Some("user")
                && !m
                    .get("content")
                    .and_then(|c| c.as_array())
                    .is_some_and(|blocks| {
                        blocks
                            .iter()
                            .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
                    })
        };
        let last_user = a.iter().rposition(is_real_user).unwrap_or(0);
        a[last_user..]
            .iter()
            .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"))
            .count() as u32
    }

    /// Decide a turn from the raw request bytes, parsing nothing.
    fn policy_bytes_script(&self, body: &[u8]) -> Option<Scripted> {
        let m = self.mode.lock().expect("fake mode").clone();
        let FakeMode::Policy {
            tool_rounds,
            parallel,
            tool,
            text_bytes,
        } = m
        else {
            return None;
        };
        Some(Self::scripted_for(
            Self::assistants_this_turn_scan(body),
            tool_rounds,
            parallel,
            &tool,
            text_bytes,
        ))
    }

    fn scripted_for(
        assistant_so_far: u32,
        tool_rounds: u32,
        parallel: usize,
        tool: &str,
        text_bytes: usize,
    ) -> Scripted {
        if assistant_so_far < tool_rounds {
            Scripted::ToolCalls(
                (0..parallel.max(1))
                    .map(|i| {
                        (
                            format!("c{assistant_so_far}_{i}"),
                            tool.to_string(),
                            serde_json::json!({ "i": i }),
                        )
                    })
                    .collect(),
            )
        } else {
            Scripted::Text("x".repeat(text_bytes))
        }
    }

    /// Decide a turn from the request alone. No shared state, so K concurrent
    /// sessions cannot interfere.
    fn policy_script(&self, body: &serde_json::Value) -> Option<Scripted> {
        let m = self.mode.lock().expect("fake mode").clone();
        let FakeMode::Policy {
            tool_rounds,
            parallel,
            tool,
            text_bytes,
        } = m
        else {
            return None;
        };
        // ONE counting function for both paths. This used to be an inline duplicate of
        // assistants_this_turn_parsed; the two drifted (the duplicate kept treating Anthropic
        // tool-result user messages as turn boundaries) and the sampled-inspection runs took
        // the buggy copy on exactly 1-in-N requests — an off-by-a-few in the round counts that
        // looked entirely plausible. The benchmark guards caught it; never duplicate this.
        let assistant_so_far = Self::assistants_this_turn_parsed(body);
        Some(Self::scripted_for(
            assistant_so_far,
            tool_rounds,
            parallel,
            &tool,
            text_bytes,
        ))
    }

    pub fn script(&self, turns: impl IntoIterator<Item = Scripted>) {
        let mut q = self.queue.lock().expect("fake queue");
        for t in turns {
            q.push_back(t);
        }
    }

    pub fn pending(&self) -> usize {
        self.queue.lock().expect("fake queue").len()
    }

    /// **Call this after every batch.** Not optional.
    pub fn assert_drained(&self, expected_calls: u64, where_: &str) -> Result<()> {
        let pending = self.pending();
        let calls = self.call_count.load(Ordering::SeqCst);
        if pending != 0 {
            return Err(BrainError::Protocol(format!(
                "{where_}: fake provider still has {pending} queued response(s); \
                 the loop made fewer calls than the script expected"
            )));
        }
        if calls != expected_calls {
            return Err(BrainError::Protocol(format!(
                "{where_}: fake provider served {calls} call(s), expected {expected_calls}"
            )));
        }
        Ok(())
    }

    pub fn arrival_count(&self) -> usize {
        self.arrivals.lock().expect("arrivals").len()
    }
}

/// Canonical request hash, matching `experiments/brain/lib/canonical.mjs`'s
/// shape so a Rust-built request and a Pi-built request are comparable.
pub fn canonical_hashes(body: &serde_json::Value) -> (String, String) {
    use sha2::{Digest, Sha256};
    let model = body
        .get("model")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let system = body
        .get("system")
        .cloned()
        .or_else(|| {
            body.get("messages")
                .and_then(|m| m.as_array())
                .and_then(|a| {
                    a.iter()
                        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
                })
                .and_then(|m| m.get("content").cloned())
        })
        .unwrap_or(serde_json::Value::Null);
    let tools: Vec<serde_json::Value> = body
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|a| {
            a.iter()
                .map(|t| {
                    // Both dialects normalise to the same triple.
                    let f = t.get("function").unwrap_or(t);
                    serde_json::json!({
                        "name": f.get("name"),
                        "description": f.get("description"),
                        "input_schema": f.get("input_schema").or_else(|| f.get("parameters")),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let messages = body
        .get("messages")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let prefix = serde_json::json!({"model": model, "system": system, "tools": tools});
    let full = serde_json::json!({"model": prefix["model"], "system": prefix["system"],
                                  "tools": prefix["tools"], "messages": messages});
    let h = |v: &serde_json::Value| {
        let mut s = Sha256::new();
        s.update(serde_json::to_vec(v).unwrap_or_default());
        hex::encode(s.finalize())
    };
    (h(&full), h(&prefix))
}

#[async_trait::async_trait]
impl Provider for FakeProvider {
    fn dialect(&self) -> Dialect {
        self.dialect
    }

    fn build_request(
        &self,
        prefix: &SealedPrefix,
        history: &[Message],
        key: &ProviderKey,
        base_url: &str,
    ) -> Result<ModelRequest> {
        // Deliberately delegates to the REAL adapter. The fake replaces the
        // transport, never the request builder -- otherwise the cold-start
        // number would be timing a function the production path does not call.
        match self.dialect {
            Dialect::AnthropicMessages => {
                super::anthropic::Anthropic.build_request(prefix, history, key, base_url)
            }
            Dialect::OpenAiChat => {
                super::openai::OpenAiChat.build_request(prefix, history, key, base_url)
            }
        }
    }

    async fn stream(
        &self,
        req: ModelRequest,
        _outbound: &crate::outbound::Outbound,
    ) -> Result<BoxStream<'static, Result<ProviderEvent>>> {
        let call_no = self.call_count.fetch_add(1, Ordering::SeqCst);

        // The fast path: decide the turn from raw bytes and record nothing.
        // Taken only when the caller has explicitly asked for sampling; the
        // default (`inspect_every == 1`) inspects every request.
        let every = self.inspect_every.load(Ordering::Relaxed).max(1);
        if every > 1 && !call_no.is_multiple_of(every) {
            let next =
                match self.policy_bytes_script(&req.body) {
                    Some(s) => s,
                    None => match self.queue.lock().expect("fake queue").pop_front() {
                        Some(s) => s,
                        None => return Err(BrainError::Protocol(
                            "fake provider exhausted: the loop asked for a turn the script did \
                             not queue"
                                .into(),
                        )),
                    },
                };
            return Ok(self.emit(next));
        }

        let body: serde_json::Value = serde_json::from_slice(&req.body)?;
        // The cross-check that keeps the fast path honest: on every inspected
        // request, the cheap byte scan must agree with the parse.
        if Self::assistants_this_turn_scan(&req.body) != Self::assistants_this_turn_parsed(&body) {
            self.policy_scan_mismatches.fetch_add(1, Ordering::SeqCst);
        }
        let (context_sha256, prefix_sha256) = canonical_hashes(&body);
        let arrival = Arrival {
            at_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            url: req.url.clone(),
            body_bytes: req.body.len(),
            message_count: body
                .get("messages")
                .and_then(|m| m.as_array())
                .map(|a| a.len())
                .unwrap_or(0),
            tools_offered: body
                .get("tools")
                .and_then(|t| t.as_array())
                .map(|a| a.len())
                .unwrap_or(0),
            system_chars: system_chars(&body),
            context_sha256,
            prefix_sha256,
        };
        {
            let mut a = self.arrivals.lock().expect("arrivals");
            // Bounded: an unbounded arrival log is a leak inside the thing being
            // measured, and at benchmark throughput it is a large one. Dropping
            // the OLDEST keeps the most recent window, which is what a guard
            // reads. The count of calls is `call_count` and is never truncated,
            // so "how many arrived" stays exact even when "which ones" does not.
            let cap = self.arrivals_cap.load(Ordering::Relaxed) as usize;
            if a.len() >= cap && cap > 0 {
                a.remove(0);
            }
            if cap > 0 {
                a.push(arrival);
            }
        }

        let next = match self.policy_script(&body) {
            Some(s) => Some(s),
            None => self.queue.lock().expect("fake queue").pop_front(),
        };
        let Some(next) = next else {
            // Unlike Pi's faux provider, exhaustion is a typed error, not a
            // plausible-looking assistant turn. Pi's choice is exactly what
            // makes `assertDrained` mandatory there; making it an error here
            // means a script bug cannot be mistaken for a result.
            return Err(BrainError::Protocol(
                "fake provider exhausted: the loop asked for a turn the script did not queue"
                    .into(),
            ));
        };

        Ok(self.emit(next))
    }
}

impl FakeProvider {
    /// Turn a scripted turn into a provider event stream.
    ///
    /// Factored out so the sampled fast path and the fully-inspected path emit
    /// **the same bytes**. Two copies of this would be two subtly different
    /// providers, and the sampling rate would then be a hidden variable in every
    /// throughput number.
    fn emit(&self, next: Scripted) -> BoxStream<'static, Result<ProviderEvent>> {
        let tps = self.tokens_per_second.load(Ordering::Relaxed);
        let s = async_stream::stream! {
            match next {
                Scripted::Text(t) => {
                    for tok in split_tokens(&t) {
                        if tps > 0 {
                            tokio::time::sleep(std::time::Duration::from_nanos(
                                1_000_000_000 / tps.max(1),
                            )).await;
                        } else {
                            // A real provider stream crosses an async socket boundary between
                            // chunks. Preserve that scheduling boundary even in unpaced tests so
                            // an instant large fake response cannot monopolize the runtime and
                            // manufacture EventHub lag that cannot occur on the network path.
                            tokio::task::yield_now().await;
                        }
                        yield Ok(ProviderEvent::TextDelta { index: 0, text: tok });
                    }
                    yield Ok(ProviderEvent::MessageDone {
                        stop_reason: StopReason::EndTurn,
                        usage: Usage { input_tokens: Some(0), output_tokens: Some(0),
                                       cache_read_input_tokens: None,
                                       cache_creation_input_tokens: None,
                                       reasoning_tokens: None },
                    });
                }
                Scripted::Refusal(text) => {
                    yield Ok(ProviderEvent::RefusalDelta { index: 0, text });
                    yield Ok(ProviderEvent::MessageDone {
                        stop_reason: StopReason::Refusal,
                        usage: Usage { input_tokens: Some(0), output_tokens: Some(0),
                                       cache_read_input_tokens: None,
                                       cache_creation_input_tokens: None,
                                       reasoning_tokens: None },
                    });
                }
                Scripted::ToolCalls(calls) => {
                    for (i, (id, name, input)) in calls.into_iter().enumerate() {
                        yield Ok(ProviderEvent::ToolUseStart { index: i + 1, id, name });
                        yield Ok(ProviderEvent::ToolInputDelta {
                            index: i + 1,
                            partial_json: serde_json::to_string(&input).unwrap_or_else(|_| "{}".into()),
                        });
                        yield Ok(ProviderEvent::BlockDone { index: i + 1 });
                    }
                    yield Ok(ProviderEvent::MessageDone {
                        stop_reason: StopReason::ToolUse,
                        usage: Usage { input_tokens: Some(0), output_tokens: Some(0),
                                       cache_read_input_tokens: None,
                                       cache_creation_input_tokens: None,
                                       reasoning_tokens: None },
                    });
                }
                Scripted::TransportError { partial_text, message } => {
                    if let Some(text) = partial_text {
                        yield Ok(ProviderEvent::TextDelta { index: 0, text });
                    }
                    yield Err(BrainError::Transport(message));
                }
                Scripted::ProviderStatus { status, body } => {
                    yield Err(BrainError::ProviderStatus { status, body });
                }
            }
        };
        Box::pin(s)
    }
}

/// Last occurrence of `needle` in `hay`.
/// System-prompt length on either wire: Anthropic's top-level `system` (string or text
/// blocks), or OpenAI's `messages[0]` with `role:"system"`.
fn system_chars(body: &serde_json::Value) -> usize {
    if let Some(s) = body.get("system") {
        if let Some(s) = s.as_str() {
            return s.len();
        }
        if let Some(blocks) = s.as_array() {
            return blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .map(str::len)
                .sum();
        }
    }
    body.get("messages")
        .and_then(|m| m.as_array())
        .and_then(|a| a.first())
        .filter(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"))
        .and_then(|m| m.get("content").and_then(|c| c.as_str()))
        .map(str::len)
        .unwrap_or(0)
}

fn find_from(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn split_tokens(s: &str) -> Vec<String> {
    // 4 bytes per "token", matching pi-faux's `tokenSize: 4` default so the
    // streaming shapes are comparable.
    s.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exhaustion_is_an_error_not_a_plausible_turn() {
        let f = FakeProvider::new(Dialect::OpenAiChat);
        let req = ModelRequest {
            method: "POST",
            url: "http://fake/v1/chat/completions".into(),
            headers: vec![],
            body: b"{\"messages\":[]}".to_vec(),
        };
        let err = match f.stream(req, &crate::outbound::Outbound::new(true)).await {
            Ok(_) => panic!("exhaustion must not produce a stream"),
            Err(e) => e,
        };
        assert!(matches!(err, BrainError::Protocol(_)));
    }

    #[test]
    fn policy_counts_rounds_within_the_current_turn_only() {
        let f = FakeProvider::new(Dialect::OpenAiChat);
        f.set_mode(FakeMode::Policy {
            tool_rounds: 2,
            parallel: 1,
            tool: "t".into(),
            text_bytes: 4,
        });
        // Turn 2 of a session: turn 1 already contributed 2 assistants and its
        // closing text. The policy must still give turn 2 its 2 tool rounds.
        let body = serde_json::json!({"messages":[
            {"role":"system","content":"s"},
            {"role":"user","content":"turn 1"},
            {"role":"assistant","tool_calls":[]},
            {"role":"tool","content":"r"},
            {"role":"assistant","tool_calls":[]},
            {"role":"tool","content":"r"},
            {"role":"assistant","content":"done"},
            {"role":"user","content":"turn 2"}
        ]});
        assert!(
            matches!(f.policy_script(&body), Some(Scripted::ToolCalls(_))),
            "turn 2 round 0 must still get tool calls"
        );
        // Same turn, one round in.
        let mut b2 = body.clone();
        b2["messages"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"role":"assistant","tool_calls":[]}));
        assert!(matches!(f.policy_script(&b2), Some(Scripted::ToolCalls(_))));
        // Two rounds in -> text.
        b2["messages"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"role":"assistant","tool_calls":[]}));
        assert!(matches!(f.policy_script(&b2), Some(Scripted::Text(_))));
    }

    #[test]
    fn assert_drained_catches_an_undrained_queue() {
        let f = FakeProvider::new(Dialect::OpenAiChat);
        f.script([Scripted::Text("a".into()), Scripted::Text("b".into())]);
        assert!(f.assert_drained(2, "t").is_err(), "2 pending must fail");
    }

    #[test]
    fn anthropic_tool_result_user_messages_are_not_turn_boundaries() {
        // On the Anthropic wire a tool result is a user-role message carrying tool_result
        // blocks. The round counter must skip those, or the policy resets to round 0 after
        // every tool round and emits tool calls until the round cap (the benchmark caught
        // exactly this: 2,560 model calls where 60 were expected).
        let body = serde_json::json!({"messages": [
            {"role": "user", "content": [{"type": "text", "text": "go"}]},
            {"role": "assistant", "content": [{"type": "tool_use", "id": "c0", "name": "bash", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "c0", "content": "ok"}]},
        ]});
        assert_eq!(FakeProvider::assistants_this_turn_parsed(&body), 1);
        let bytes = serde_json::to_vec(&body).unwrap();
        assert_eq!(FakeProvider::assistants_this_turn_scan(&bytes), 1);
        // A REAL user message after the tool round starts a new turn.
        let mut b2 = body.clone();
        b2["messages"].as_array_mut().unwrap().extend([
            serde_json::json!({"role": "assistant", "content": [{"type": "text", "text": "done"}]}),
            serde_json::json!({"role": "user", "content": [{"type": "text", "text": "next"}]}),
        ]);
        assert_eq!(FakeProvider::assistants_this_turn_parsed(&b2), 0);
        let bytes2 = serde_json::to_vec(&b2).unwrap();
        assert_eq!(FakeProvider::assistants_this_turn_scan(&bytes2), 0);
    }
}
