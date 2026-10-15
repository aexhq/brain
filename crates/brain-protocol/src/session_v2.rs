//! Language-neutral contracts for the SQL-authoritative session server.
//! Sequences are decimal strings so JavaScript cannot round record identities.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

macro_rules! object {
    ($name:ident { $($fields:tt)* }) => {
        #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $($fields)* }
    };
}

macro_rules! choice {
    ($name:ident { $($variants:ident),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variants),* }
    };
}

choice!(Lifecycle {
    Open,
    Closing,
    Closed,
    Deleted
});
choice!(RunStatus {
    Queued,
    Active,
    Waiting,
    Paused,
    Completed,
    Failed,
    Cancelled,
    Blocked
});
choice!(OperationStatus {
    Prepared,
    Dispatched,
    Completed,
    Failed,
    Unknown,
    Cancelled
});
choice!(InteractionKind {
    Question,
    Form,
    Approval
});
choice!(InteractionStatus {
    Pending,
    Answered,
    Rejected,
    Expired,
    Cancelled
});
choice!(Disposition {
    Application,
    State,
    Context,
    Wake,
    Steer,
    Interrupt
});
choice!(ControlKind {
    Pause,
    Resume,
    Cancel,
    Close
});
choice!(ConnectionKind {
    Application,
    Http,
    Local,
    Modal,
    Fly,
    Mcp,
    Storage
});
choice!(Ownership { Managed, Borrowed });
choice!(Outcome {
    Completed,
    Failed,
    Unknown
});
choice!(GapKind {
    Reserved,
    Unused,
    Unavailable,
    Unresolved
});
choice!(RuntimeLanguage { Node, Python });
choice!(EffectKind {
    Model,
    Action,
    Build,
    Lifecycle,
    Webhook
});
choice!(ErrorCode {
    Invalid,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    Overloaded,
    Unsupported,
    Unavailable,
    Uncertain,
    Corrupt,
    Internal
});

object!(ErrorResponse {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
});

object!(ModelSelection {
    pub provider: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<Value>,
    #[serde(default)]
    pub native: BTreeMap<String, Value>,
});

object!(HarnessSelection {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub configuration: BTreeMap<String, Value>,
});

object!(Limits {
    pub calls: u32,
    pub concurrent: u32,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub retained_bytes: u64,
    pub pending_inputs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
});

object!(Action {
    pub name: String,
    pub description: String,
    pub connection: String,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    pub max_output_bytes: u64,
    pub timeout_ms: u64,
    #[serde(default)]
    pub approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
});

object!(ConnectionTemplate {
    pub name: String,
    pub kind: ConnectionKind,
    pub provider: String,
    pub ownership: Ownership,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub configuration: BTreeMap<String, Value>,
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
});

object!(EventRoute {
    pub name: String,
    pub version: String,
    pub schema: Value,
    pub disposition: Disposition,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
});

object!(SessionConfig {
    pub model: ModelSelection,
    pub harness: HarnessSelection,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub connections: Vec<ConnectionTemplate>,
    #[serde(default)]
    pub events: Vec<EventRoute>,
    #[serde(default)]
    pub harness_transitions: Vec<HarnessSelection>,
    #[serde(default)]
    pub grants: Vec<String>,
    pub limits: Limits,
});

object!(SessionCreate {
    pub request_id: String,
    pub config: SessionConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
});

object!(SessionView {
    pub id: String,
    pub lifecycle: Lifecycle,
    pub revision: String,
    pub sequence: String,
    pub config: SessionConfig,
    pub created_ms: u64,
    pub owner_epoch: String,
    pub paused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
});

object!(SessionPage {
    pub sessions: Vec<SessionView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
});

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputData {
    Message {
        text: String,
    },
    Event {
        name: String,
        version: String,
        data: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        correlation: Option<String>,
    },
    Action {
        name: String,
        arguments: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_of: Option<String>,
    },
    Control {
        control: ControlKind,
    },
}

object!(Input {
    pub request_id: String,
    pub data: InputData,
});

object!(CommandReceipt {
    pub id: String,
    pub session_id: String,
    pub status: String,
    pub accepted_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorResponse>,
});

object!(Record {
    pub session_id: String,
    pub sequence: String,
    pub kind: String,
    pub time_ms: u64,
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
});

object!(RecordReference {
    pub session_id: String,
    pub sequence: String,
    pub digest: String,
});

object!(Gap {
    pub first: String,
    pub last: String,
    pub kind: GapKind,
});

object!(RecordPage {
    pub records: Vec<Record>,
    pub gaps: Vec<Gap>,
    pub allocated_through: String,
    pub next: String,
    pub more: bool,
});

object!(Run {
    pub id: String,
    pub session_id: String,
    pub command_id: String,
    pub status: RunStatus,
    pub step: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<Wait>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<RecordReference>,
});

choice!(ChildResultAvailability {
    Pending,
    Inline,
    Referenced,
    Unavailable,
    None,
});

object!(ChildResult {
    pub index: u32,
    pub session_id: String,
    pub command_id: String,
    pub run_id: String,
    pub status: RunStatus,
    pub availability: ChildResultAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<RecordReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorResponse>,
});

object!(ChildResults {
    pub session_id: String,
    pub run_id: String,
    pub step: u32,
    pub expected: u32,
    pub ready: bool,
    pub children: Vec<ChildResult>,
});

object!(Wait {
    pub kind: String,
    pub id: String,
});

object!(UnavailablePayload {
    #[schemars(regex(pattern = "^[a-f0-9]{64}$"))]
    pub digest: String,
    pub bytes: u64,
});

object!(Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
    #[serde(default)]
    pub provider: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_unavailable: Option<UnavailablePayload>,
});

object!(Operation {
    pub id: String,
    pub session_id: String,
    pub run_id: String,
    pub step_key: String,
    pub kind: EffectKind,
    pub action: String,
    pub arguments_digest: String,
    pub status: OperationStatus,
    pub recipient: String,
    pub revision: String,
    pub result_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_of: Option<String>,
});

object!(Claim {
    pub request_id: String,
    pub connection_id: String,
    pub generation: String,
});

object!(Execution {
    pub operation: Operation,
    pub arguments: Value,
    pub receipt_token: String,
    pub max_output_bytes: u64,
    pub deadline_ms: u64,
});

object!(Receipt {
    #[schemars(length(min = 1, max = 128))]
    pub request_id: String,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
});

object!(Retry {
    pub request_id: String,
    pub acknowledge_uncertainty: bool,
});

object!(InteractionCreate {
    pub request_id: String,
    pub kind: InteractionKind,
    pub run_id: String,
    pub subject: String,
    pub subject_digest: String,
    pub prompt: String,
    pub schema: Value,
    pub expires_ms: u64,
});

object!(Interaction {
    pub id: String,
    pub session_id: String,
    pub revision: String,
    pub status: InteractionStatus,
    pub request: InteractionCreate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Value>,
});

object!(InteractionAnswer {
    pub request_id: String,
    pub revision: String,
    pub subject_digest: String,
    pub accept: bool,
    pub answer: Value,
});

object!(ConnectionAttach {
    pub request_id: String,
    pub template: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
});

object!(Connection {
    pub id: String,
    pub session_id: String,
    pub template: String,
    pub generation: String,
    pub status: String,
    pub ownership: Ownership,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
});

object!(DocumentUpdate {
    pub request_id: String,
    pub revision: String,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_ms: Option<u64>,
});

object!(Document {
    pub key: String,
    pub revision: String,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_ms: Option<u64>,
});

object!(FileInfo {
    pub name: String,
    pub digest: String,
    pub size: u64,
    pub media_type: String,
    pub provenance: String,
});

object!(ScheduleCreate {
    pub request_id: String,
    pub due_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
    pub input: InputData,
});

object!(Schedule {
    pub id: String,
    pub session_id: String,
    pub due_ms: u64,
    pub status: String,
    pub request: ScheduleCreate,
});

object!(Delegation {
    pub request_id: String,
    pub config: SessionConfig,
    pub message: String,
    pub context: Vec<String>,
});

object!(Handoff {
    pub request_id: String,
    pub revision: String,
    pub harness: HarnessSelection,
});

object!(NodeInfo {
    pub id: String,
    pub endpoint: String,
    pub lease_expires_ms: u64,
    pub draining: bool,
});

choice!(TransferPhase {
    Draining,
    Verifying,
    Transferred,
    Cancelled,
    Failed
});

object!(TransferCreate {
    pub request_id: String,
    pub owner_epoch: String,
    pub target_node: String,
});

object!(Transfer {
    pub id: String,
    pub session_id: String,
    pub source_node: String,
    pub source_epoch: String,
    pub target_node: String,
    pub phase: TransferPhase,
    pub through: String,
    pub verified_runs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<RecordReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorResponse>,
});

object!(IdentityIssuer {
    pub issuer: String,
    pub audience: String,
    pub jwks_url: String,
    pub permissions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_claim: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_token_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_ms: Option<u64>,
});

object!(IdentityView {
    pub tenant: String,
    pub subject: String,
    pub permissions: Vec<String>,
    pub operator: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_ms: Option<u64>,
});

object!(ParticipantGrant {
    pub request_id: String,
    pub subject: String,
    pub permissions: Vec<String>,
    pub expires_ms: u64,
});

object!(ParticipantToken {
    pub id: String,
    pub token: String,
    pub expires_ms: u64,
});

object!(PackageManifest {
    pub name: String,
    pub version: String,
    pub language: RuntimeLanguage,
    pub entrypoint: String,
    pub files: BTreeMap<String, String>,
    pub lockfile: String,
    pub actions: Vec<Action>,
    pub secrets: Vec<String>,
    pub network: Vec<String>,
    pub memory_bytes: u64,
    pub cpu_millis: u32,
    pub wall_ms: u64,
});

object!(PackageAdmission {
    pub request_id: String,
    pub session_id: String,
    pub manifest: PackageManifest,
    pub builder: String,
});

object!(Package {
    pub id: String,
    pub digest: String,
    pub status: String,
    pub manifest: PackageManifest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
});

object!(ManagedInvocation {
    pub execution: Execution,
    pub package: Package,
    pub callback_url: String,
    pub secrets: BTreeMap<String, String>,
});

object!(PackageBuildResult {
    pub package_id: String,
    pub source_digest: String,
    pub image: String,
    pub builder: String,
    pub base_image: String,
});

object!(CatalogEntry {
    pub name: String,
    pub version: String,
    pub kind: String,
    pub capabilities: Vec<String>,
    pub credentials: Vec<String>,
    pub limitations: Vec<String>,
    pub configuration_schema: Value,
});

object!(ModelMessage {
    pub role: String,
    pub content: Value,
    #[serde(default)]
    pub tool_calls: Vec<ModelToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
});

object!(ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
});

object!(ModelRequest {
    pub messages: Vec<ModelMessage>,
    pub actions: Vec<Action>,
});

object!(ModelResponse {
    pub message: ModelMessage,
    pub usage: Usage,
    #[serde(default)]
    pub native: BTreeMap<String, Value>,
});

object!(HarnessRequest {
    pub protocol: String,
    pub session_id: String,
    pub run_id: String,
    pub command_id: String,
    pub step: u32,
    pub config: SessionConfig,
    pub input: InputData,
    pub context: Vec<Record>,
    pub checkpoint: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Operation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ChildResult>,
});

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decision {
    Call {
        effect: EffectKind,
        action: String,
        arguments: Value,
        checkpoint: Value,
    },
    Complete {
        result: Value,
        checkpoint: Value,
    },
    Wait {
        wait: Wait,
        checkpoint: Value,
    },
    Emit {
        name: String,
        version: String,
        data: Value,
        checkpoint: Value,
    },
    Interact {
        interaction: InteractionCreate,
        checkpoint: Value,
    },
    Delegate {
        delegation: Box<Delegation>,
        checkpoint: Value,
    },
    Fork {
        #[schemars(length(min = 1, max = 8))]
        delegations: Vec<Delegation>,
        checkpoint: Value,
    },
}

object!(SessionBudget {
    pub session_id: String,
    pub calls: u64,
    pub active_operations: u64,
    pub retained_bytes: u64,
    pub reserved_output_bytes: u64,
    pub reported_tokens: String,
    pub reserved_tokens: String,
    pub incomplete_usages: u64,
    pub token_usage_pending: bool,
    pub remaining_calls: u64,
    pub available_operations: u64,
    pub available_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_tokens: Option<String>,
});

object!(StorageReport {
    pub format: String,
    pub retained_bytes: u64,
    pub cache_bytes: u64,
    pub unarchived_bytes: u64,
    pub reserved_output_bytes: u64,
    pub archive_queue_bytes: u64,
    pub oldest_unarchived_ms: u64,
    pub sessions: u64,
    pub accepting: bool,
});

choice!(ExportSection {
    History,
    Runs,
    Operations,
    Interactions,
    Connections,
    Files,
    State,
    Memory,
    Schedules
});

object!(ExportManifest {
    pub format: String,
    pub section: ExportSection,
    pub session: SessionView,
    pub runs: Vec<Run>,
    pub operations: Vec<Operation>,
    pub interactions: Vec<Interaction>,
    pub connections: Vec<Connection>,
    pub files: Vec<FileInfo>,
    pub state: Vec<Document>,
    pub memory: Vec<Document>,
    pub schedules: Vec<Schedule>,
    pub history: RecordPage,
    pub complete: bool,
    pub continuation: String,
});
