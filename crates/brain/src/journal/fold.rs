use super::*;

// ---------------------------------------------------------------------------------------------
// Fold
// ---------------------------------------------------------------------------------------------

/// The model-visible history rebuilt from records. `fold` is a loop over `apply` so the cold
/// (rehydrate) and hot (in-turn append) paths cannot drift.
#[derive(Debug, Default, Clone)]
pub struct Fold {
    pub history: Vec<Message>,
    /// Consecutive tool_result records group into one user message (Anthropic requires tool
    /// results to arrive as one user message per batch); flushed by the next non-result record.
    pending_results: Vec<ContentBlock>,
    pub turns: u64,
}

impl Fold {
    /// Resumes a fold from an already-rebuilt history (in-turn compaction).
    pub fn from_history(history: Vec<Message>) -> Self {
        Fold {
            history,
            pending_results: Vec::new(),
            turns: 0,
        }
    }

    pub fn apply(&mut self, record: &Record) {
        // Subagent records (slice 8) never enter the ROOT history: a child's assistant
        // message is not the parent's, and -- load-bearing -- a child record landing
        // between two root tool results of one batch must not flush them into separate
        // user messages (providers require one user message per result batch). The
        // parent's own `task` ToolCall/ToolResult carry the parent's agent id and fold
        // normally.
        if let Some(agent) = record.agent()
            && agent != "root"
        {
            return;
        }
        match record {
            Record::UserMessage {
                content,
                starts_turn,
                ..
            } => {
                if *starts_turn {
                    self.turns += 1;
                }
                if self.pending_results.is_empty() {
                    self.history.push(Message {
                        role: Role::User,
                        content: content.clone(),
                    });
                } else {
                    // A recovered/cancelled turn may end immediately after its
                    // tool results. Merge the next real user text into that same
                    // user message so provider histories still alternate roles.
                    let mut merged = std::mem::take(&mut self.pending_results);
                    merged.extend(content.clone());
                    self.history.push(Message {
                        role: Role::User,
                        content: merged,
                    });
                }
            }
            Record::TurnStarted { .. } => self.turns += 1,
            Record::Assistant { content, .. } => {
                self.flush_results();
                self.history.push(Message {
                    role: Role::Assistant,
                    content: content.clone(),
                });
            }
            Record::ToolResult {
                call,
                content,
                is_error,
                ..
            } => {
                self.pending_results.push(ContentBlock::ToolResult {
                    tool_use_id: call.clone(),
                    content: content.clone(),
                    is_error: *is_error,
                });
            }
            Record::Usage { .. }
            | Record::ModelCallIntent { .. }
            | Record::ModelCallUnknown { .. }
            | Record::ModelAttemptSuperseded { .. }
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
            | Record::ToolCall { .. }
            | Record::TurnCompleted { .. }
            | Record::TurnFailed { .. }
            | Record::State { .. }
            | Record::EnvironmentLost { .. }
            | Record::StorageUploadReserved { .. }
            | Record::StorageUploadPublished { .. }
            | Record::StorageUploadCompleted { .. }
            | Record::StorageUploadExpired { .. }
            | Record::StorageDeleteIntent { .. }
            | Record::StorageDeleteCompleted { .. }
            | Record::SandboxFileEffectIntent { .. }
            | Record::SandboxFileEffectCompleted { .. }
            | Record::DefaultSandboxChanged { .. }
            | Record::ContextChunk { .. }
            | Record::ContextInstalled { .. }
            // Loop-land state is never kernel model input; contract loops compose their own
            // provider context from marks and the journal_read projections.
            | Record::LoopCustom { .. }
            | Record::LoopEvent { .. }
            | Record::LoopMark { .. }
            | Record::LoopKvSet { .. } => {}
        }
    }

    fn flush_results(&mut self) {
        if !self.pending_results.is_empty() {
            self.history.push(Message::tool_results(std::mem::take(
                &mut self.pending_results,
            )));
        }
    }

    /// Terminal flush: called once all records are applied.
    pub fn finish(&mut self) {
        self.flush_results();
    }
}

pub fn fold(entries: &[Entry]) -> Fold {
    let mut f = Fold::default();
    for e in entries {
        f.apply(&e.record);
    }
    f.finish();
    f
}
