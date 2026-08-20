//! Linear, prefix-stable compaction (D11).
//!
//! When the model-visible history outgrows its byte budget, everything but a trailing window
//! is replaced by one summary message. Two properties are load-bearing:
//!
//! - **prefix-stable**: the sealed prefix (system prompt, tools, model) is never touched, so
//!   the provider's cache entry for it survives compaction;
//! - **linear**: one pass over the history, O(n) once, not O(n^2) over the session's life
//!   (Pi's quadratic variant measured 13 s at 1,000 turns).
//!
//! The cut lands on a turn boundary: the kept tail always starts with a plain user message,
//! never with a tool-result message (whose `tool_use` partner would be gone) and never with
//! an assistant message (which the dialects reject as a conversation opener).

use crate::message::{ContentBlock, Message, Role};

/// Default in-memory history budget before a compaction is journaled. Well under every
/// certified model's context window; the provider remains the authority on token limits.
pub const DEFAULT_HISTORY_BUDGET_BYTES: usize = 512 * 1024;

/// How much history a compaction keeps, as a fraction of the budget.
const KEEP_FRACTION: f64 = 0.5;

fn message_bytes(m: &Message) -> usize {
    m.heap_bytes()
}

/// True if this message can open the kept tail: a user message with no tool_result blocks.
fn is_turn_opener(m: &Message) -> bool {
    m.role == Role::User
        && m.content
            .iter()
            .all(|b| !matches!(b, ContentBlock::ToolResult { .. }))
}

/// The compaction decision: `Some((summary, kept))` when the history is over budget and a
/// valid cut exists. `kept` counts messages from the end; the summary replaces the rest.
pub fn plan(history: &[Message], budget_bytes: usize) -> Option<(String, u64)> {
    let total: usize = history.iter().map(message_bytes).sum();
    if total <= budget_bytes {
        return None;
    }
    let keep_budget = (budget_bytes as f64 * KEEP_FRACTION) as usize;

    // Walk back to the newest turn-opener under the keep budget.
    let mut acc = 0usize;
    let mut cut: Option<usize> = None; // index of the first kept message
    for (i, m) in history.iter().enumerate().rev() {
        acc += message_bytes(m);
        if acc > keep_budget {
            break;
        }
        if is_turn_opener(m) {
            cut = Some(i);
        }
    }
    let cut = cut?;
    if cut == 0 {
        return None; // everything already fits from the first turn opener
    }
    let dropped = cut;
    let summary = format!(
        "[Earlier context was compacted: {dropped} messages from previous turns were elided. \
         The workspace on disk is unaffected and remains the source of truth.]"
    );
    Some((summary, (history.len() - cut) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{Record, fold};
    use crate::message::Message;

    fn turn(n: usize, bulk: usize) -> Vec<Message> {
        vec![
            Message::user_text(format!("request {n}: {}", "x".repeat(bulk))),
            Message::assistant(vec![ContentBlock::ToolUse {
                id: format!("c{n}"),
                name: "bash".into(),
                input: serde_json::json!({"command": "true"}),
            }]),
            Message::tool_results(vec![ContentBlock::ToolResult {
                tool_use_id: format!("c{n}"),
                content: "y".repeat(bulk),
                is_error: false,
            }]),
            Message::assistant(vec![ContentBlock::text(format!("done {n}"))]),
        ]
    }

    #[test]
    fn under_budget_is_left_alone() {
        let history: Vec<Message> = turn(1, 10);
        assert!(plan(&history, 1 << 20).is_none());
    }

    #[test]
    fn over_budget_cuts_on_a_turn_opener_and_keeps_pairs_together() {
        let mut history = Vec::new();
        for n in 0..20 {
            history.extend(turn(n, 4096));
        }
        let (summary, kept) = plan(&history, 64 * 1024).expect("must compact");
        assert!(kept > 0 && (kept as usize) < history.len());
        let tail = &history[history.len() - kept as usize..];
        assert!(
            is_turn_opener(&tail[0]),
            "kept tail must start a turn, got {:?}",
            tail[0].role
        );
        // Every tool_result in the tail has its tool_use in the tail too.
        for (i, m) in tail.iter().enumerate() {
            for b in &m.content {
                if let ContentBlock::ToolResult { tool_use_id, .. } = b {
                    let paired = tail[..i]
                        .iter()
                        .any(|prev| prev.tool_uses().any(|(id, _, _)| id == tool_use_id));
                    assert!(paired, "orphan tool_result {tool_use_id}");
                }
            }
        }
        assert!(summary.contains("compacted"));
    }

    #[test]
    fn fold_applies_the_plan_exactly_as_the_planner_meant_it() {
        let mut history = Vec::new();
        for n in 0..12 {
            history.extend(turn(n, 4096));
        }
        let (summary, kept) = plan(&history, 64 * 1024).unwrap();
        // Replay through the journal fold: same tail, summary in front.
        let mut f = crate::journal::Fold::default();
        for m in &history {
            // Reconstruct records from messages for the test.
            match m.role {
                Role::User => {
                    if is_turn_opener(m) {
                        f.apply(&Record::UserMessage {
                            turn: "t".into(),
                            content: m.content.clone(),
                            metadata: std::collections::HashMap::new(),
                        });
                    } else {
                        for b in &m.content {
                            if let ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                is_error,
                            } = b
                            {
                                f.apply(&Record::ToolResult {
                                    turn: "t".into(),
                                    agent: "root".into(),
                                    call: tool_use_id.clone(),
                                    name: "bash".into(),
                                    outcome: "completed".into(),
                                    content: content.clone(),
                                    is_error: *is_error,
                                    exit_code: None,
                                    duration_ms: 0,
                                    truncated: false,
                                });
                            }
                        }
                    }
                }
                Role::Assistant => f.apply(&Record::Assistant {
                    turn: "t".into(),
                    agent: "root".into(),
                    content: m.content.clone(),
                    stop: crate::message::StopReason::EndTurn,
                }),
            }
        }
        f.apply(&Record::Compacted {
            summary: summary.clone(),
            kept,
        });
        f.finish();
        assert_eq!(f.history.len(), kept as usize + 1);
        assert_eq!(f.history[0], Message::user_text(summary));
        assert_eq!(&f.history[1..], &history[history.len() - kept as usize..]);
        let _ = fold(&[]); // keep the import honest
    }
}
