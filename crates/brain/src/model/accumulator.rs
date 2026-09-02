//! Accumulates a dialect-neutral event stream into one complete assistant
//! message.
//!
//! A provider response becomes model-visible history **only after a complete
//! assistant message commits**. Partial deltas are journaled as stream events
//! but never become the message the agentloop observes.

use brain_protocol::{ContentBlock, Message, ModelStreamEvent, StopReason, Usage};

use crate::Error;

pub const MAX_PROVIDER_ASSISTANT_BYTES: usize = 192 * 1024;
pub const MAX_PROVIDER_DELTA_BYTES: usize = 64 * 1024;
pub const MAX_PROVIDER_CONTENT_BLOCKS: usize = 64;
pub const MAX_PROVIDER_TOOL_CALLS: usize = 32;

#[derive(Debug, Default)]
pub struct Accumulator {
    blocks: Vec<PartialBlock>,
    stop_reason: StopReason,
    usage: Usage,
    saw_terminal: bool,
    saw_refusal: bool,
    total_bytes: usize,
    tool_calls: usize,
}

#[derive(Debug)]
enum PartialBlock {
    Empty,
    Text {
        text: String,
        done: bool,
    },
    Tool {
        id: String,
        name: String,
        json: String,
        done: bool,
    },
}

impl Accumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn saw_terminal(&self) -> bool {
        self.saw_terminal
    }

    pub fn push(&mut self, ev: ModelStreamEvent) -> Result<(), Error> {
        if self.saw_terminal && !matches!(ev, ModelStreamEvent::Usage { .. }) {
            return Err(protocol(
                "provider emitted content or a second terminal event after message completion"
                    .into(),
            ));
        }
        let (index, added_bytes, starts_tool) = match &ev {
            ModelStreamEvent::TextDelta { index, text }
            | ModelStreamEvent::RefusalDelta { index, text } => (*index, text.len(), false),
            ModelStreamEvent::ToolUseStart { index, id, name } => {
                (*index, id.len().saturating_add(name.len()), true)
            }
            ModelStreamEvent::ToolInputDelta {
                index,
                partial_json,
            } => (*index, partial_json.len(), false),
            ModelStreamEvent::BlockDone { index } => (*index, 0, false),
            ModelStreamEvent::Usage { .. } | ModelStreamEvent::MessageDone { .. } => (0, 0, false),
        };
        if !matches!(
            ev,
            ModelStreamEvent::Usage { .. } | ModelStreamEvent::MessageDone { .. }
        ) && index >= MAX_PROVIDER_CONTENT_BLOCKS
        {
            return Err(protocol(format!(
                "provider content block index {index} exceeds {}",
                MAX_PROVIDER_CONTENT_BLOCKS - 1
            )));
        }
        if added_bytes > MAX_PROVIDER_DELTA_BYTES {
            return Err(protocol(format!(
                "provider delta exceeds {MAX_PROVIDER_DELTA_BYTES} bytes"
            )));
        }
        if self.total_bytes.saturating_add(added_bytes) > MAX_PROVIDER_ASSISTANT_BYTES {
            return Err(protocol(format!(
                "provider assistant content exceeds {MAX_PROVIDER_ASSISTANT_BYTES} bytes"
            )));
        }
        if starts_tool && self.tool_calls >= MAX_PROVIDER_TOOL_CALLS {
            return Err(protocol(format!(
                "provider returned more than {MAX_PROVIDER_TOOL_CALLS} tool calls"
            )));
        }
        match ev {
            ModelStreamEvent::TextDelta { index, text } => self.push_text(index, text)?,
            ModelStreamEvent::RefusalDelta { index, text } => {
                self.saw_refusal = true;
                self.push_text(index, text)?;
            }
            ModelStreamEvent::ToolUseStart { index, id, name } => {
                self.ensure(index);
                if !matches!(self.blocks[index], PartialBlock::Empty) {
                    return Err(protocol(format!(
                        "provider started content block {index} more than once"
                    )));
                }
                self.blocks[index] = PartialBlock::Tool {
                    id,
                    name,
                    json: String::new(),
                    done: false,
                };
            }
            ModelStreamEvent::ToolInputDelta {
                index,
                partial_json,
            } => {
                self.ensure(index);
                match &mut self.blocks[index] {
                    PartialBlock::Tool {
                        json, done: false, ..
                    } => json.push_str(&partial_json),
                    PartialBlock::Tool { done: true, .. } => {
                        return Err(protocol(format!(
                            "provider emitted tool JSON for completed block {index}"
                        )));
                    }
                    PartialBlock::Empty => {
                        return Err(protocol(format!(
                            "provider emitted tool JSON before starting block {index}"
                        )));
                    }
                    PartialBlock::Text { .. } => return Err(block_type_conflict(index)),
                }
            }
            ModelStreamEvent::BlockDone { index } => {
                self.ensure(index);
                match &mut self.blocks[index] {
                    PartialBlock::Text { done, .. } | PartialBlock::Tool { done, .. } if !*done => {
                        *done = true
                    }
                    PartialBlock::Empty => {
                        return Err(protocol(format!(
                            "provider completed absent content block {index}"
                        )));
                    }
                    _ => {
                        return Err(protocol(format!(
                            "provider completed content block {index} more than once"
                        )));
                    }
                }
            }
            ModelStreamEvent::Usage { usage } => self.merge_usage(&usage)?,
            ModelStreamEvent::MessageDone { stop_reason, usage } => {
                // OpenAI emits usage in a final choices=[] chunk. Its unknown stop
                // reason must not overwrite the real finish reason that preceded it.
                if stop_reason != StopReason::Unknown || self.stop_reason == StopReason::Unknown {
                    self.stop_reason = stop_reason;
                }
                self.merge_usage(&usage)?;
                self.saw_terminal = true;
            }
        }
        self.total_bytes = self.total_bytes.saturating_add(added_bytes);
        self.tool_calls += usize::from(starts_tool);
        Ok(())
    }

    fn push_text(&mut self, index: usize, text: String) -> Result<(), Error> {
        self.ensure(index);
        match &mut self.blocks[index] {
            PartialBlock::Empty => {
                self.blocks[index] = PartialBlock::Text { text, done: false };
            }
            PartialBlock::Text {
                text: value,
                done: false,
            } => value.push_str(&text),
            PartialBlock::Text { done: true, .. } => {
                return Err(protocol(format!(
                    "provider emitted text for completed block {index}"
                )));
            }
            PartialBlock::Tool { .. } => return Err(block_type_conflict(index)),
        }
        Ok(())
    }

    fn merge_usage(&mut self, usage: &Usage) -> Result<(), Error> {
        self.usage
            .merge(usage)
            .map_err(|message| protocol(message.into()))
    }

    fn ensure(&mut self, index: usize) {
        while self.blocks.len() <= index {
            self.blocks.push(PartialBlock::Empty);
        }
    }

    /// Finish into a message. Empty text blocks are dropped; a tool block whose
    /// accumulated JSON does not parse is surfaced as a typed error rather than
    /// being coerced to `{}` -- coercing would let the model's call silently
    /// become a different call.
    pub fn finish(self) -> Result<(Message, StopReason, Usage), Error> {
        let mut content = Vec::with_capacity(self.blocks.len());
        for (index, block) in self.blocks.into_iter().enumerate() {
            match block {
                // OpenAI reserves index zero for text and starts tool calls at one.
                // A pure tool-call response has exactly this one intentional gap.
                PartialBlock::Empty if index == 0 => {}
                PartialBlock::Empty => {
                    return Err(protocol(
                        "provider left a gap in its content block indexes".into(),
                    ));
                }
                PartialBlock::Text { text, .. } if text.is_empty() => {}
                PartialBlock::Text { text, .. } => content.push(ContentBlock::Text { text }),
                PartialBlock::Tool { id, name, json, .. } => {
                    let input: serde_json::Value = if json.trim().is_empty() {
                        serde_json::Value::Object(Default::default())
                    } else {
                        serde_json::from_str(&json).map_err(|error| {
                            protocol(format!(
                                "tool_use {name} ({id}) input is not valid JSON after {} bytes of deltas: {error}",
                                json.len()
                            ))
                        })?
                    };
                    content.push(ContentBlock::ToolUse { id, name, input });
                }
            }
        }
        let stop_reason = if self.saw_refusal {
            StopReason::Refusal
        } else {
            self.stop_reason
        };
        Ok((Message::assistant(content), stop_reason, self.usage))
    }
}

fn block_type_conflict(index: usize) -> Error {
    protocol(format!(
        "provider changed the type of content block {index}"
    ))
}

/// A provider that violates its own stream protocol mid-stream leaves the
/// operation's billing outcome unknown, which is exactly what `Ambiguous`
/// means to the journal.
fn protocol(message: String) -> Error {
    Error::Ambiguous(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_broken_tool_json_instead_of_coercing() {
        let mut a = Accumulator::new();
        a.push(ModelStreamEvent::ToolUseStart {
            index: 0,
            id: "t1".into(),
            name: "read".into(),
        })
        .unwrap();
        a.push(ModelStreamEvent::ToolInputDelta {
            index: 0,
            partial_json: "{\"path\": \"/etc/pas".into(),
        })
        .unwrap();
        a.push(ModelStreamEvent::MessageDone {
            stop_reason: StopReason::ToolUse,
            usage: Usage::default(),
        })
        .unwrap();
        let error = a.finish().unwrap_err();
        assert!(
            matches!(error, Error::Ambiguous(_)),
            "a truncated tool input must be a typed error, got {error:?}"
        );
    }

    #[test]
    fn joins_split_json_deltas_and_absent_usage_stays_absent() {
        let mut a = Accumulator::new();
        a.push(ModelStreamEvent::ToolUseStart {
            index: 0,
            id: "t1".into(),
            name: "read".into(),
        })
        .unwrap();
        for chunk in ["{\"pa", "th\": \"/e", "tc/hosts\"}"] {
            a.push(ModelStreamEvent::ToolInputDelta {
                index: 0,
                partial_json: chunk.into(),
            })
            .unwrap();
        }
        a.push(ModelStreamEvent::MessageDone {
            stop_reason: StopReason::ToolUse,
            usage: Usage {
                input_tokens: Some(10),
                output_tokens: Some(2),
                ..Usage::default()
            },
        })
        .unwrap();
        let (message, stop, usage) = a.finish().unwrap();
        assert_eq!(stop, StopReason::ToolUse);
        assert_eq!(usage.cache_read_input_tokens, None);
        let uses: Vec<_> = message.tool_uses().collect();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].1, "read");
        assert_eq!(uses[0].2["path"], "/etc/hosts");
    }

    #[test]
    fn rejects_usage_overflow_without_wrapping() {
        let mut a = Accumulator::new();
        a.push(ModelStreamEvent::Usage {
            usage: Usage {
                input_tokens: Some(u64::MAX),
                ..Usage::default()
            },
        })
        .unwrap();
        let error = a
            .push(ModelStreamEvent::Usage {
                usage: Usage {
                    input_tokens: Some(1),
                    ..Usage::default()
                },
            })
            .unwrap_err();
        assert!(matches!(error, Error::Ambiguous(_)));
        assert_eq!(a.usage.input_tokens, Some(u64::MAX));
    }

    #[test]
    fn refusal_survives_an_ordinary_stop_reason() {
        let mut a = Accumulator::new();
        a.push(ModelStreamEvent::RefusalDelta {
            index: 0,
            text: "I cannot help with that.".into(),
        })
        .unwrap();
        a.push(ModelStreamEvent::MessageDone {
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
        })
        .unwrap();
        let (message, stop, _) = a.finish().unwrap();
        assert_eq!(stop, StopReason::Refusal);
        assert!(matches!(
            &message.content[0],
            ContentBlock::Text { text } if text == "I cannot help with that."
        ));
    }

    #[test]
    fn rejects_type_changes_duplicates_and_post_terminal_deltas() {
        let mut type_change = Accumulator::new();
        type_change
            .push(ModelStreamEvent::TextDelta {
                index: 0,
                text: "text".into(),
            })
            .unwrap();
        assert!(
            type_change
                .push(ModelStreamEvent::ToolInputDelta {
                    index: 0,
                    partial_json: "{}".into(),
                })
                .is_err()
        );

        let mut duplicate = Accumulator::new();
        let start = || ModelStreamEvent::ToolUseStart {
            index: 1,
            id: "call_1".into(),
            name: "read".into(),
        };
        duplicate.push(start()).unwrap();
        assert!(duplicate.push(start()).is_err());

        let mut completed = Accumulator::new();
        completed
            .push(ModelStreamEvent::TextDelta {
                index: 0,
                text: "done".into(),
            })
            .unwrap();
        completed
            .push(ModelStreamEvent::BlockDone { index: 0 })
            .unwrap();
        assert!(
            completed
                .push(ModelStreamEvent::TextDelta {
                    index: 0,
                    text: "late".into(),
                })
                .is_err()
        );

        let mut terminal = Accumulator::new();
        terminal
            .push(ModelStreamEvent::MessageDone {
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
            })
            .unwrap();
        assert!(
            terminal
                .push(ModelStreamEvent::TextDelta {
                    index: 0,
                    text: "late".into(),
                })
                .is_err()
        );
        assert!(
            terminal
                .push(ModelStreamEvent::Usage {
                    usage: Usage::default(),
                })
                .is_ok(),
            "a trailing usage-only frame after the terminal is legal"
        );
    }
}
