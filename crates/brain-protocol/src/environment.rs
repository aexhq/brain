use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AttachmentId, EnvironmentId, Identity, Outcome, Resources, Runtime, SessionId, ToolManifest,
};

/// The contract identifier every command and response carries.
pub const ENVIRONMENT_CONTRACT: &str = "environment/v1";

/// What a client asks for when it creates an environment.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateEnvironmentRequest {
    /// The id the environment is known by. Minted by Brain when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<EnvironmentId>,
    pub configuration: serde_json::Value,
    /// Whether Brain closes the environment once no session has been attached to it
    /// for `idle_ttl_ms`. An unmanaged environment lives until it is deleted.
    #[serde(default)]
    pub managed: bool,
    /// Absent means the server's default; zero means never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_ttl_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentStatus {
    Open,
    /// The last operation could not reach the environment. Cleared by the next one that
    /// does.
    Unreachable,
}

/// What the API says about an environment.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EnvironmentSummary {
    pub environment_id: EnvironmentId,
    pub status: EnvironmentStatus,
    pub managed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_ttl_ms: Option<u64>,
    /// Sessions attached right now.
    pub attached_sessions: Vec<SessionId>,
    /// What the environment declared it executes and offers at setup.
    #[serde(default)]
    pub runtimes: Vec<Runtime>,
    /// What the environment declared, verbatim. Brain reads the names; the policy
    /// blocks are the environment contract's business.
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::json_object")]
    pub resources: Resources,
    pub created_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct EnvironmentList {
    pub environments: Vec<EnvironmentSummary>,
}

/// What a session create names about one environment it attaches to. `bindings` carries
/// plaintext values for the environment's hosted tools and exists only here: the
/// configuration the session journals never carries them.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentAttachRequest {
    pub environment_id: EnvironmentId,
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
    /// The wire binding, present once the environment has been resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<EnvironmentBinding>,
    /// Present once the environment has attached this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<AttachmentId>,
    /// What the environment's setup/attach receipts declared it executes. Kept with the
    /// session so the bind check holds however the session was admitted.
    #[serde(default)]
    pub runtimes: Vec<Runtime>,
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
    /// receiver that already answered it can say so. An operation on the environment
    /// itself rather than on a session's behalf carries the environment's own id here.
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

/// One provisioned-tool artifact carried at attach: the manifest the environment reads
/// and the content identity naming the payload it must already hold or fetch.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provision {
    pub manifest: ToolManifest,
    pub payload_identity: Identity,
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
        /// The program runtimes this environment launches, reported on setup/attach
        /// receipts and fed into the bind-time check.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[schemars(extend("uniqueItems" = true))]
        runtimes: Vec<Runtime>,
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
    Ambiguous {
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
