//! The codes that cross Brain's boundaries.
//!
//! Three closed sets: the kinds a journal record can have, the codes a failed turn or a
//! failed effect can carry, and the codes an API error can carry. Every producer names its
//! code from here so that a client, an agentloop, or a test can match on a string that is
//! declared exactly once. `contracts/session/v1/codes.json` mirrors this module for the SDK
//! and the documentation; a conformance test keeps the two identical.

use serde::{Deserialize, Serialize};

/// Journal record kinds.
pub mod event {
    pub const SESSION_CREATION_STARTED: &str = "session_creation_started";
    pub const SESSION_CREATION_ENDED: &str = "session_creation_ended";
    pub const SESSION_CREATION_FAILED: &str = "session_creation_failed";
    pub const SESSION_END_STARTED: &str = "session_end_started";
    pub const SESSION_END_FAILED: &str = "session_end_failed";
    pub const SESSION_ENDED: &str = "session_ended";
    pub const SESSION_SUSPENDED: &str = "session_suspended";
    pub const SESSION_RESUMED: &str = "session_resumed";
    pub const TURN_STARTED: &str = "turn_started";
    pub const TURN_ENDED: &str = "turn_ended";
    pub const TURN_FAILED: &str = "turn_failed";
    pub const ACTIVATION_STARTED: &str = "activation_started";
    pub const ACTIVATION_ENDED: &str = "activation_ended";
    pub const ACTIVATION_FAILED: &str = "activation_failed";
    pub const TRANSCRIPT_REPLACED: &str = "transcript_replaced";
    pub const MODEL_CALL_STARTED: &str = "model_call_started";
    pub const MODEL_CALL_ENDED: &str = "model_call_ended";
    pub const MODEL_CALL_FAILED: &str = "model_call_failed";
    pub const TOOL_CALL_STARTED: &str = "tool_call_started";
    pub const TOOL_CALL_ENDED: &str = "tool_call_ended";
    pub const TOOL_CALL_FAILED: &str = "tool_call_failed";
    pub const TOOL_CANCEL_STARTED: &str = "tool_cancel_started";
    pub const TOOL_CANCEL_ENDED: &str = "tool_cancel_ended";
    pub const TOOL_CANCEL_FAILED: &str = "tool_cancel_failed";
    pub const OUTPUT_EMITTED: &str = "output_emitted";
    pub const ENVIRONMENT_SETUP_STARTED: &str = "environment_setup_started";
    pub const ENVIRONMENT_SETUP_ENDED: &str = "environment_setup_ended";
    pub const ENVIRONMENT_SETUP_FAILED: &str = "environment_setup_failed";
    pub const ENVIRONMENT_ATTACH_STARTED: &str = "environment_attach_started";
    pub const ENVIRONMENT_ATTACH_ENDED: &str = "environment_attach_ended";
    pub const ENVIRONMENT_ATTACH_FAILED: &str = "environment_attach_failed";
    pub const ENVIRONMENT_CALL_STARTED: &str = "environment_call_started";
    pub const ENVIRONMENT_CALL_ENDED: &str = "environment_call_ended";
    pub const ENVIRONMENT_CALL_FAILED: &str = "environment_call_failed";
    pub const ENVIRONMENT_DETACH_STARTED: &str = "environment_detach_started";
    pub const ENVIRONMENT_DETACH_ENDED: &str = "environment_detach_ended";
    pub const ENVIRONMENT_DETACH_FAILED: &str = "environment_detach_failed";
    pub const ENVIRONMENT_TEARDOWN_STARTED: &str = "environment_teardown_started";
    pub const ENVIRONMENT_TEARDOWN_ENDED: &str = "environment_teardown_ended";
    pub const ENVIRONMENT_TEARDOWN_FAILED: &str = "environment_teardown_failed";
    pub const ENVIRONMENT_CLOSED: &str = "environment_closed";
    pub const ENVIRONMENT_UNREACHABLE: &str = "environment_unreachable";

    /// The prefixes an effect record is named by; `_started`, `_ended` and `_failed` are
    /// appended by the session.
    pub mod call {
        pub const MODEL_CALL: &str = "model_call";
        pub const TOOL_CALL: &str = "tool_call";
        pub const TOOL_CANCEL: &str = "tool_cancel";
        pub const ENVIRONMENT_SETUP: &str = "environment_setup";
        pub const ENVIRONMENT_ATTACH: &str = "environment_attach";
        pub const ENVIRONMENT_CALL: &str = "environment_call";
        pub const ENVIRONMENT_DETACH: &str = "environment_detach";
        pub const ENVIRONMENT_TEARDOWN: &str = "environment_teardown";
    }

    pub const ALL: &[&str] = &[
        SESSION_CREATION_STARTED,
        SESSION_CREATION_ENDED,
        SESSION_CREATION_FAILED,
        SESSION_END_STARTED,
        SESSION_END_FAILED,
        SESSION_ENDED,
        SESSION_SUSPENDED,
        SESSION_RESUMED,
        TURN_STARTED,
        TURN_ENDED,
        TURN_FAILED,
        ACTIVATION_STARTED,
        ACTIVATION_ENDED,
        ACTIVATION_FAILED,
        TRANSCRIPT_REPLACED,
        MODEL_CALL_STARTED,
        MODEL_CALL_ENDED,
        MODEL_CALL_FAILED,
        TOOL_CALL_STARTED,
        TOOL_CALL_ENDED,
        TOOL_CALL_FAILED,
        TOOL_CANCEL_STARTED,
        TOOL_CANCEL_ENDED,
        TOOL_CANCEL_FAILED,
        OUTPUT_EMITTED,
        ENVIRONMENT_SETUP_STARTED,
        ENVIRONMENT_SETUP_ENDED,
        ENVIRONMENT_SETUP_FAILED,
        ENVIRONMENT_ATTACH_STARTED,
        ENVIRONMENT_ATTACH_ENDED,
        ENVIRONMENT_ATTACH_FAILED,
        ENVIRONMENT_CALL_STARTED,
        ENVIRONMENT_CALL_ENDED,
        ENVIRONMENT_CALL_FAILED,
        ENVIRONMENT_DETACH_STARTED,
        ENVIRONMENT_DETACH_ENDED,
        ENVIRONMENT_DETACH_FAILED,
        ENVIRONMENT_TEARDOWN_STARTED,
        ENVIRONMENT_TEARDOWN_ENDED,
        ENVIRONMENT_TEARDOWN_FAILED,
        ENVIRONMENT_CLOSED,
        ENVIRONMENT_UNREACHABLE,
    ];
}

/// Why a turn, an effect, or a session creation failed.
pub mod failure {
    pub const INTERRUPTED: &str = "interrupted";
    pub const CANCELLED: &str = "cancelled";
    pub const TIMEOUT: &str = "timeout";
    pub const UNKNOWN: &str = "unknown";
    pub const MODEL_CALL_LIMIT: &str = "model_call_limit";
    pub const EMIT_LIMIT: &str = "emit_limit";
    pub const AGENTLOOP_FAILED: &str = "agentloop_failed";
    pub const TOOL_ERROR: &str = "tool_error";
    pub const SESSION_OWNERSHIP_FAILED: &str = "session_ownership_failed";
    pub const ENVIRONMENT_PREPARATION_FAILED: &str = "environment_preparation_failed";
    pub const ENVIRONMENT_ERROR: &str = "environment_error";
    pub const INVALID_TRANSCRIPT: &str = "invalid_transcript";

    pub const ALL: &[&str] = &[
        INTERRUPTED,
        CANCELLED,
        TIMEOUT,
        UNKNOWN,
        MODEL_CALL_LIMIT,
        EMIT_LIMIT,
        AGENTLOOP_FAILED,
        TOOL_ERROR,
        SESSION_OWNERSHIP_FAILED,
        ENVIRONMENT_PREPARATION_FAILED,
        ENVIRONMENT_ERROR,
        INVALID_TRANSCRIPT,
    ];
}

/// What an API response can fail with.
pub mod api {
    pub const INVALID_REQUEST: &str = "invalid_request";
    pub const UNAUTHORIZED: &str = "unauthorized";
    pub const NOT_FOUND: &str = "not_found";
    pub const CONFLICT: &str = "conflict";
    pub const OVERLOADED: &str = "overloaded";
    pub const AMBIGUOUS: &str = "ambiguous";
    pub const EXECUTOR_FAILED: &str = "executor_failed";
    pub const MODEL_PROVIDER_FAILED: &str = "model_provider_failed";
    pub const INTERNAL: &str = "internal";

    pub const ALL: &[&str] = &[
        INVALID_REQUEST,
        UNAUTHORIZED,
        NOT_FOUND,
        CONFLICT,
        OVERLOADED,
        AMBIGUOUS,
        EXECUTOR_FAILED,
        MODEL_PROVIDER_FAILED,
        INTERNAL,
    ];
}

/// The one payload every `*_failed` record carries. `ambiguous` says whether the effect
/// may have happened anyway; `retryable` is the producer's advice, not a promise.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Failure {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default)]
    pub ambiguous: bool,
}

impl Failure {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
            ambiguous: false,
        }
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn ambiguous(mut self, ambiguous: bool) -> Self {
        self.ambiguous = ambiguous;
        self
    }
}

/// The whole catalogue, for the conformance test and for `codes.json`.
pub fn catalogue() -> serde_json::Value {
    serde_json::json!({
        "events": event::ALL,
        "failures": failure::ALL,
        "api": api::ALL,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_is_an_identifier_and_unique() {
        for set in [event::ALL, failure::ALL, api::ALL] {
            let mut seen = std::collections::HashSet::new();
            for code in set {
                assert!(
                    code.bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                    "{code} is not lowercase snake case"
                );
                assert!(seen.insert(*code), "{code} is listed twice");
            }
        }
    }
}
