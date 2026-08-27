use serde::{Deserialize, Serialize};

use crate::{Event, ModelRequest, ToolInvocation};

pub const AGENTLOOP_CONTRACT_VERSION: &str = "agentloop/v1";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ContextEnvelope {
    pub protocol_version: String,
    pub items: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Observation {
    SessionStarted,
    UserMessage { content: serde_json::Value },
    ModelCompleted { response: serde_json::Value },
    ToolsCompleted { results: Vec<serde_json::Value> },
    Emitted { event: Event },
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Presentation {
    pub bytes: Vec<u8>,
    pub digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeEnvelope {
    pub logical_time_ms: u64,
    pub deterministic_seed: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActivationInput {
    pub context: ContextEnvelope,
    pub observation: Observation,
    pub presentation: Presentation,
    pub runtime: RuntimeEnvelope,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActivationOutput {
    pub context: ContextEnvelope,
    pub decision: Decision,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Decision {
    Model {
        request: ModelRequest,
    },
    Tools {
        calls: Vec<ToolInvocation>,
    },
    Emit {
        event: serde_json::Value,
    },
    Finish {
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
    },
    Fail {
        code: String,
        message: String,
        retryable: bool,
    },
}
