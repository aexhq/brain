use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{AttachmentId, EnvironmentId, Outcome, Resources, SessionId, ToolManifest};

/// The contract identifier every command and response carries.
pub const ENVIRONMENT_CONTRACT: &str = "environment/v1";

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentStatus {
    /// No observation has been made in this process; metadata does not restore a resource.
    Unknown,
    Open,
    /// The last operation could not reach the environment. Cleared by the next one that
    /// does.
    Unreachable,
}

/// One immutable Environment specification in a session create. `bindings` carries
/// plaintext values only until attach and is never copied into the session journal.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEnvironment {
    pub environment_id: EnvironmentId,
    pub configuration: serde_json::Value,
    #[serde(default)]
    #[schemars(with = "crate::schema::BindingValues")]
    pub bindings: BTreeMap<String, String>,
}

/// How an environment is addressed on its wire.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EnvironmentBinding {
    pub environment_id: EnvironmentId,
    #[schemars(range(min = 1))]
    pub directory_generation: u64,
}

/// One environment a session was granted: what the create request named, and once the
/// host has attached it, what the environment answered with.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnvironmentAttachment {
    pub environment_id: EnvironmentId,
    pub configuration: serde_json::Value,
    /// The wire binding, present once the environment has been resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<EnvironmentBinding>,
    /// Present once the environment has attached this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<AttachmentId>,
    /// The resources the environment declared, verbatim. Brain reads the names.
    #[serde(default)]
    pub resources: Resources,
}

impl EnvironmentAttachment {
    /// Whether the host has attached this environment yet.
    pub fn attached(&self) -> bool {
        self.binding.is_some()
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EnvironmentOperation {
    /// The sequence of the journal record that started this operation. With
    /// `session_id` it names the operation: a redelivery carries the same pair, so a
    /// receiver that already answered it can say so. Setup and teardown belong to the
    /// owning session's journal too.
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub environment_id: EnvironmentId,
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<AttachmentId>,
    pub request: EnvironmentRequest,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EnvironmentCommand {
    #[schemars(schema_with = "crate::schema::environment_contract")]
    pub contract: String,
    pub binding: EnvironmentBinding,
    pub operation: EnvironmentOperation,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EnvironmentResponse {
    #[schemars(schema_with = "crate::schema::environment_contract")]
    pub contract: String,
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub receipt: EnvironmentReceipt,
}

/// One placed Tool handed to an Environment at attach.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provision {
    pub manifest: ToolManifest,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnvironmentRequest {
    Setup {
        configuration: serde_json::Value,
    },
    Attach {
        #[schemars(length(max = 128))]
        provisions: Vec<Provision>,
        /// Binding values by name, injected into hosted tools at runtime. Plaintext on
        /// this wire only: the journal never holds the values.
        #[schemars(with = "crate::schema::BindingValues")]
        bindings: BTreeMap<String, String>,
    },
    Call {
        #[schemars(schema_with = "crate::schema::identifier")]
        name: String,
        input: serde_json::Value,
    },
    Invoke {
        #[schemars(schema_with = "crate::schema::identifier")]
        call_id: String,
        #[schemars(schema_with = "crate::schema::identifier")]
        tool: String,
        input: serde_json::Value,
        #[schemars(range(min = 1))]
        deadline_ms: u64,
    },
    Cancel {
        #[schemars(range(min = 1))]
        target_sequence: u64,
    },
    Detach,
    Teardown,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnvironmentReceipt {
    Accepted {
        /// The resources this environment declares, reported on setup/attach receipts
        /// and fed into the bind-time `needs ⊆ resources` check.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        #[schemars(with = "crate::schema::ResourcePolicies")]
        resources: Resources,
    },
    Progress {
        data: serde_json::Value,
    },
    Result {
        output: serde_json::Value,
    },
    Outcome {
        outcome: Outcome,
    },
    Failure {
        #[schemars(schema_with = "crate::schema::identifier")]
        code: String,
        #[schemars(length(max = 4096))]
        message: String,
        retryable: bool,
    },
    Unknown {
        #[schemars(length(max = 4096))]
        message: String,
    },
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentCallRequest {
    pub input: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentCallResult {
    pub output: serde_json::Value,
}
