use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    AttachmentId, Capability, EnvironmentId, GrantSet, Identity, OperationId, Outcome, SessionId,
    ToolManifest,
};

/// The contract identifier every command and response carries.
pub const ENVIRONMENT_CONTRACT: &str = "environment/v2";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecyclePolicy {
    Session,
    Shared,
    External,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnvironmentBinding {
    pub environment_id: EnvironmentId,
    pub configuration_identity: Identity,
    pub directory_generation: u64,
    pub lifecycle_policy: LifecyclePolicy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnvironmentAttachment {
    pub binding: EnvironmentBinding,
    pub attachment_id: AttachmentId,
    /// What the environment's setup/attach receipts said it provides. Sealed with the
    /// session so the capability check holds however the session was admitted.
    #[serde(default)]
    pub provides: Vec<Capability>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnvironmentOperation<T> {
    pub operation_id: OperationId,
    pub request_identity: Identity,
    pub environment_id: EnvironmentId,
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<AttachmentId>,
    pub request: T,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnvironmentCommand<T> {
    pub contract: String,
    pub binding: EnvironmentBinding,
    pub operation: EnvironmentOperation<T>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnvironmentResponse {
    pub contract: String,
    pub operation_id: OperationId,
    pub request_identity: Identity,
    pub receipt: EnvironmentReceipt,
}

/// One provisioned-tool artifact carried at attach: the manifest the environment reads
/// and the content identity naming the payload it must already hold or fetch.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provision {
    pub manifest: ToolManifest,
    pub payload_identity: Identity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnvironmentRequest {
    Setup {
        configuration: serde_json::Value,
    },
    Attach {
        grants: GrantSet,
        provisions: Vec<Provision>,
        /// Binding values by name, injected into hosted tools at runtime. Plaintext on
        /// this wire only: the journal carries their identities, never the values.
        bindings: BTreeMap<String, String>,
    },
    Call {
        name: String,
        input: serde_json::Value,
    },
    Invoke {
        call_id: String,
        tool: String,
        input: serde_json::Value,
        deadline_ms: u64,
    },
    Cancel {
        target_operation_id: OperationId,
    },
    Detach,
    Teardown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EnvironmentReceipt {
    Accepted {
        /// The capabilities this environment provides, reported on setup/attach
        /// receipts and fed into the bind-time `requires ⊆ provides` check.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        provides: Vec<Capability>,
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
        code: String,
        message: String,
        retryable: bool,
    },
    Conflict {
        expected_identity: Identity,
        actual_identity: Identity,
    },
    Ambiguous {
        message: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentCallRequest {
    pub input: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentCallResult {
    pub output: serde_json::Value,
}
