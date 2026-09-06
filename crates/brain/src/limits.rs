use std::time::Duration;

/// The budgets a session runs under. Brain ships a default for each and never treats it
/// as a fact about the machine: the host that starts Brain sets them. Zero on any
/// ceiling means no bound, except the live backlog, which must be at least 1.
#[derive(Clone, Debug, clap::Args)]
pub struct Limits {
    /// Model calls one turn may make before the next fails with `model_call_limit`.
    #[arg(long, env = "BRAIN_MAX_MODEL_CALLS", default_value_t = Limits::default().max_model_calls)]
    pub max_model_calls: usize,
    /// Seconds one turn may run before it is cancelled.
    #[arg(long, env = "BRAIN_MAX_TURN_SECS", default_value_t = Limits::default().max_turn_secs)]
    pub max_turn_secs: u64,
    /// Seconds one Tool call may run before the session records a `timeout` outcome.
    #[arg(long, env = "BRAIN_MAX_TOOL_SECS", default_value_t = Limits::default().max_tool_secs)]
    pub max_tool_secs: u64,
    /// Bytes of extension Events one turn may emit, kinds and payloads together.
    #[arg(long, env = "BRAIN_MAX_EMITTED_BYTES", default_value_t = Limits::default().max_emitted_bytes)]
    pub max_emitted_bytes: usize,
    /// Bytes of assistant content one model call may return.
    #[arg(long, env = "BRAIN_MAX_MODEL_OUTPUT_BYTES", default_value_t = Limits::default().max_model_output_bytes)]
    pub max_model_output_bytes: usize,
    /// Bytes one streamed model delta may carry.
    #[arg(long, env = "BRAIN_MAX_MODEL_DELTA_BYTES", default_value_t = Limits::default().max_model_delta_bytes)]
    pub max_model_delta_bytes: usize,
    /// Bytes one model response stream may carry in total.
    #[arg(long, env = "BRAIN_MAX_MODEL_STREAM_BYTES", default_value_t = Limits::default().max_model_stream_bytes)]
    pub max_model_stream_bytes: usize,
    /// Bytes one server-sent event frame from a model provider may carry.
    #[arg(long, env = "BRAIN_MAX_MODEL_FRAME_BYTES", default_value_t = Limits::default().max_model_frame_bytes)]
    pub max_model_frame_bytes: usize,
    /// Bytes of a provider error body kept for the failure record.
    #[arg(long, env = "BRAIN_MAX_MODEL_ERROR_BYTES", default_value_t = Limits::default().max_model_error_bytes)]
    pub max_model_error_bytes: usize,
    /// Seconds one model call may take, connection to last byte.
    #[arg(long, env = "BRAIN_MAX_MODEL_SECS", default_value_t = Limits::default().max_model_secs)]
    pub max_model_secs: u64,
    /// Seconds connecting to a model provider may take.
    #[arg(long, env = "BRAIN_MAX_MODEL_CONNECT_SECS", default_value_t = Limits::default().max_model_connect_secs)]
    pub max_model_connect_secs: u64,
    /// Bytes the journal writer may hold queued across every session before appends wait.
    #[arg(long, env = "BRAIN_MAX_JOURNAL_QUEUE_BYTES", default_value_t = Limits::default().max_journal_queue_bytes)]
    pub max_journal_queue_bytes: u64,
    /// Bytes the journal writer may hold queued for one session before its appends wait.
    #[arg(long, env = "BRAIN_MAX_SESSION_QUEUE_BYTES", default_value_t = Limits::default().max_session_queue_bytes)]
    pub max_session_queue_bytes: u64,
    /// Journal segment files the writer keeps open at once.
    #[arg(long, env = "BRAIN_MAX_JOURNAL_OPEN_FILES", default_value_t = Limits::default().max_journal_open_files)]
    pub max_journal_open_files: usize,
    /// Records a live subscriber may fall behind before it loses them; at least 1.
    #[arg(long, env = "BRAIN_MAX_LIVE_BACKLOG", default_value_t = Limits::default().max_live_backlog)]
    pub max_live_backlog: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_model_calls: 128,
            // A turn is minutes of tool work at most, not hours.
            max_turn_secs: 30 * 60,
            // Long enough for real tool work, short enough that a hung environment
            // cannot pin a turn forever.
            max_tool_secs: 120,
            max_emitted_bytes: 1024 * 1024,
            // A 64k-token answer is about 256 KiB of text; this leaves room for
            // several times that before a long answer fails the turn.
            max_model_output_bytes: 4 * 1024 * 1024,
            max_model_delta_bytes: 64 * 1024,
            max_model_stream_bytes: 32 * 1024 * 1024,
            max_model_frame_bytes: 256 * 1024,
            max_model_error_bytes: 16 * 1024,
            max_model_secs: 120,
            max_model_connect_secs: 10,
            max_journal_queue_bytes: 64 * 1024 * 1024,
            max_session_queue_bytes: 8 * 1024 * 1024,
            max_journal_open_files: 256,
            max_live_backlog: 1_024,
        }
    }
}

impl Limits {
    /// How long a turn may run, or `None` for no bound.
    pub fn max_turn(&self) -> Option<Duration> {
        secs(self.max_turn_secs)
    }

    /// The deadline handed to every Tool call, in milliseconds. No bound is a deadline
    /// far enough away that no dispatch reaches it.
    pub fn tool_deadline_ms(&self) -> u64 {
        match secs(self.max_tool_secs) {
            Some(deadline) => deadline.as_millis() as u64,
            None => u64::MAX / 4,
        }
    }

    pub fn max_model(&self) -> Option<Duration> {
        secs(self.max_model_secs)
    }

    pub fn max_model_connect(&self) -> Option<Duration> {
        secs(self.max_model_connect_secs)
    }

    /// The live backlog is a preallocated channel and cannot be unbounded.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_live_backlog == 0 {
            return Err("BRAIN_MAX_LIVE_BACKLOG must be at least 1".into());
        }
        Ok(())
    }
}

fn secs(value: u64) -> Option<Duration> {
    (value != 0).then(|| Duration::from_secs(value))
}

/// A byte or count ceiling as the code compares against it: zero means no bound.
pub fn ceiling(value: usize) -> usize {
    if value == 0 { usize::MAX } else { value }
}

/// [`ceiling`] for the journal's byte counts.
pub fn ceiling_u64(value: u64) -> u64 {
    if value == 0 { u64::MAX } else { value }
}
