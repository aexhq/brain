//! The vocabulary between a Tool and the Environment that executes it.
//!
//! An environment is a place that executes Tools and offers resources. A tool
//! declares its implementation; the Environment prepares it and enforces configured access. Brain journals every call and never wraps the platform: inside the
//! environment a program reaches its resources through the platform's own APIs, and
//! policy is enforced at the platform boundary, not by Brain.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The one envelope every tool invocation resolves to.
///
/// `timeout` is distinguished from `error` because the deadline is caller-owned: no
/// backend family can be trusted to enforce one remotely, so the caller kills and says
/// exactly what happened rather than encoding it as an exit code.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Ok { value: serde_json::Value },
    Error { error: OutcomeError },
    Timeout,
    Cancelled,
    Unknown { message: String },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeError {
    #[schemars(schema_with = "crate::schema::identifier")]
    pub code: String,
    #[schemars(length(max = 4096))]
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outcome_carries_its_status_on_the_wire() {
        let ok = serde_json::to_value(&Outcome::Ok {
            value: serde_json::json!({"exit_code": 0}),
        })
        .unwrap();
        assert_eq!(ok["status"], "ok");
        let timeout: Outcome =
            serde_json::from_value(serde_json::json!({"status":"timeout"})).unwrap();
        assert_eq!(timeout, Outcome::Timeout);
    }
}
