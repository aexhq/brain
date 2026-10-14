//! Language-neutral process contract for a harness selected at Brain startup.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

use crate::{ModelDef, SessionId, TurnInput};

pub const HARNESS_PROTOCOL: &str = "brain/harness/v1";

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessCommand {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub configuration: serde_json::Value,
}

/// One installed implementation. Only the deployment supplies commands and capacity.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentloopRegistration {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub configuration: serde_json::Value,
    #[serde(default = "one_worker")]
    pub workers: NonZeroUsize,
    #[serde(default = "worker_concurrency")]
    pub max_concurrency: NonZeroUsize,
}

fn one_worker() -> NonZeroUsize {
    NonZeroUsize::new(1).unwrap()
}
fn worker_concurrency() -> NonZeroUsize {
    NonZeroUsize::new(64).unwrap()
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessInitialize {
    pub protocol: String,
    pub state_directory: String,
    pub configuration: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessIdentity {
    pub protocol: String,
    /// Stable adapter and native-state format identity. Changes require a new data directory.
    pub name: String,
    pub state_version: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessSession {
    pub session_id: SessionId,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessRun {
    pub session_id: SessionId,
    pub sequence: u64,
    pub model: ModelDef,
    pub input: TurnInput,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessCallback {
    pub session_id: SessionId,
    pub sequence: u64,
    pub method: String,
    pub input: serde_json::Value,
}
