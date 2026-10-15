/* eslint-disable */
/** Generated from Brain-owned v2 contracts. Do not edit. */

/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ChildResultAvailability".
 */
export type ChildResultAvailability = "pending" | "inline" | "referenced" | "unavailable" | "none";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ErrorCode".
 */
export type ErrorCode =
  | "invalid"
  | "unauthorized"
  | "forbidden"
  | "not_found"
  | "conflict"
  | "overloaded"
  | "unsupported"
  | "unavailable"
  | "uncertain"
  | "corrupt"
  | "internal";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "RunStatus".
 */
export type RunStatus =
  "queued" | "active" | "waiting" | "paused" | "completed" | "failed" | "cancelled" | "blocked";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Ownership".
 */
export type Ownership = "managed" | "borrowed";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ConnectionKind".
 */
export type ConnectionKind = "application" | "http" | "local" | "modal" | "fly" | "mcp" | "storage";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ControlKind".
 */
export type ControlKind = "pause" | "resume" | "cancel" | "close";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Decision".
 */
export type Decision =
  | {
      action: string;
      arguments: unknown;
      checkpoint: unknown;
      effect: EffectKind;
      kind: "call";
    }
  | {
      checkpoint: unknown;
      kind: "complete";
      result: unknown;
    }
  | {
      checkpoint: unknown;
      kind: "wait";
      wait: Wait;
    }
  | {
      checkpoint: unknown;
      data: unknown;
      kind: "emit";
      name: string;
      version: string;
    }
  | {
      checkpoint: unknown;
      interaction: InteractionCreate;
      kind: "interact";
    }
  | {
      checkpoint: unknown;
      delegation: Delegation;
      kind: "delegate";
    }
  | {
      checkpoint: unknown;
      /**
       * @minItems 1
       * @maxItems 8
       */
      delegations:
        | [Delegation]
        | [Delegation, Delegation]
        | [Delegation, Delegation, Delegation]
        | [Delegation, Delegation, Delegation, Delegation]
        | [Delegation, Delegation, Delegation, Delegation, Delegation]
        | [Delegation, Delegation, Delegation, Delegation, Delegation, Delegation]
        | [Delegation, Delegation, Delegation, Delegation, Delegation, Delegation, Delegation]
        | [
            Delegation,
            Delegation,
            Delegation,
            Delegation,
            Delegation,
            Delegation,
            Delegation,
            Delegation
          ];
      kind: "fork";
    };
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "EffectKind".
 */
export type EffectKind = "model" | "action" | "build" | "lifecycle" | "webhook";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "InteractionKind".
 */
export type InteractionKind = "question" | "form" | "approval";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Disposition".
 */
export type Disposition = "application" | "state" | "context" | "wake" | "steer" | "interrupt";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "OperationStatus".
 */
export type OperationStatus =
  "prepared" | "dispatched" | "completed" | "failed" | "unknown" | "cancelled";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "GapKind".
 */
export type GapKind = "reserved" | "unused" | "unavailable" | "unresolved";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "InteractionStatus".
 */
export type InteractionStatus = "pending" | "answered" | "rejected" | "expired" | "cancelled";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "InputData".
 */
export type InputData =
  | {
      kind: "message";
      text: string;
    }
  | {
      correlation?: string;
      data: unknown;
      kind: "event";
      name: string;
      version: string;
    }
  | {
      arguments: unknown;
      kind: "action";
      name: string;
      retry_of?: string;
    }
  | {
      control: ControlKind;
      kind: "control";
    };
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ExportSection".
 */
export type ExportSection =
  | "history"
  | "runs"
  | "operations"
  | "interactions"
  | "connections"
  | "files"
  | "state"
  | "memory"
  | "schedules";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Lifecycle".
 */
export type Lifecycle = "open" | "closing" | "closed" | "deleted";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "RuntimeLanguage".
 */
export type RuntimeLanguage = "node" | "python";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Outcome".
 */
export type Outcome = "completed" | "failed" | "unknown";
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "TransferPhase".
 */
export type TransferPhase = "draining" | "verifying" | "transferred" | "cancelled" | "failed";

export interface BrainSessionAPIV2 {
  contract: "brain-session/2";
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Action".
 */
export interface Action {
  approval?: boolean;
  connection: string;
  description: string;
  input_schema: unknown;
  max_output_bytes: number;
  name: string;
  output_schema?: unknown;
  package?: string;
  timeout_ms: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "CatalogEntry".
 */
export interface CatalogEntry {
  capabilities: string[];
  configuration_schema: unknown;
  credentials: string[];
  kind: string;
  limitations: string[];
  name: string;
  version: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ChildResult".
 */
export interface ChildResult {
  availability: ChildResultAvailability;
  command_id: string;
  completion?: RecordReference;
  error?: ErrorResponse;
  index: number;
  result?: unknown;
  run_id: string;
  session_id: string;
  status: RunStatus;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "RecordReference".
 */
export interface RecordReference {
  digest: string;
  sequence: string;
  session_id: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ErrorResponse".
 */
export interface ErrorResponse {
  code: ErrorCode;
  message: string;
  request_id?: string;
  retryable: boolean;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ChildResults".
 */
export interface ChildResults {
  children: ChildResult[];
  expected: number;
  ready: boolean;
  run_id: string;
  session_id: string;
  step: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Claim".
 */
export interface Claim {
  connection_id: string;
  generation: string;
  request_id: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "CommandReceipt".
 */
export interface CommandReceipt {
  accepted_ms: number;
  error?: ErrorResponse;
  id: string;
  run_id?: string;
  sequence?: string;
  session_id: string;
  status: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Connection".
 */
export interface Connection {
  generation: string;
  id: string;
  ownership: Ownership;
  resource_id?: string;
  session_id: string;
  status: string;
  template: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ConnectionAttach".
 */
export interface ConnectionAttach {
  request_id: string;
  resource_id?: string;
  template: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ConnectionTemplate".
 */
export interface ConnectionTemplate {
  capabilities?: string[];
  configuration?: {
    [k: string]: unknown | undefined;
  };
  kind: ConnectionKind;
  name: string;
  ownership: Ownership;
  provider: string;
  secrets?: {
    [k: string]: string | undefined;
  };
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Wait".
 */
export interface Wait {
  id: string;
  kind: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "InteractionCreate".
 */
export interface InteractionCreate {
  expires_ms: number;
  kind: InteractionKind;
  prompt: string;
  request_id: string;
  run_id: string;
  schema: unknown;
  subject: string;
  subject_digest: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Delegation".
 */
export interface Delegation {
  config: SessionConfig;
  context: string[];
  message: string;
  request_id: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "SessionConfig".
 */
export interface SessionConfig {
  actions?: Action[];
  connections?: ConnectionTemplate[];
  events?: EventRoute[];
  grants?: string[];
  harness: HarnessSelection;
  harness_transitions?: HarnessSelection[];
  instructions?: string;
  limits: Limits;
  model: ModelSelection;
  skills?: string[];
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "EventRoute".
 */
export interface EventRoute {
  action?: string;
  disposition: Disposition;
  name: string;
  schema: unknown;
  sources?: string[];
  version: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "HarnessSelection".
 */
export interface HarnessSelection {
  configuration?: {
    [k: string]: unknown | undefined;
  };
  name: string;
  version: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Limits".
 */
export interface Limits {
  calls: number;
  concurrent: number;
  deadline_ms?: number;
  input_bytes: number;
  output_bytes: number;
  pending_inputs: number;
  retained_bytes: number;
  tokens?: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ModelSelection".
 */
export interface ModelSelection {
  credential?: string;
  format?: unknown;
  name: string;
  native?: {
    [k: string]: unknown | undefined;
  };
  provider: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Document".
 */
export interface Document {
  expires_ms?: number;
  key: string;
  revision: string;
  value: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "DocumentUpdate".
 */
export interface DocumentUpdate {
  expires_ms?: number;
  request_id: string;
  revision: string;
  value: unknown;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Execution".
 */
export interface Execution {
  arguments: unknown;
  deadline_ms: number;
  max_output_bytes: number;
  operation: Operation;
  receipt_token: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Operation".
 */
export interface Operation {
  action: string;
  arguments_digest: string;
  error?: string;
  id: string;
  kind: EffectKind;
  recipient: string;
  result?: unknown;
  result_available: boolean;
  retry_of?: string;
  revision: string;
  run_id: string;
  session_id: string;
  status: OperationStatus;
  step_key: string;
  usage?: Usage;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Usage".
 */
export interface Usage {
  cached_tokens?: number;
  input_tokens?: number;
  output_tokens?: number;
  provider?: {
    [k: string]: unknown | undefined;
  };
  provider_unavailable?: UnavailablePayload;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "UnavailablePayload".
 */
export interface UnavailablePayload {
  bytes: number;
  digest: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ExportManifest".
 */
export interface ExportManifest {
  complete: boolean;
  connections: Connection[];
  continuation: string;
  files: FileInfo[];
  format: string;
  history: RecordPage;
  interactions: Interaction[];
  memory: Document[];
  operations: Operation[];
  runs: Run[];
  schedules: Schedule[];
  section: ExportSection;
  session: SessionView;
  state: Document[];
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "FileInfo".
 */
export interface FileInfo {
  digest: string;
  media_type: string;
  name: string;
  provenance: string;
  size: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "RecordPage".
 */
export interface RecordPage {
  allocated_through: string;
  gaps: Gap[];
  more: boolean;
  next: string;
  records: Record[];
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Gap".
 */
export interface Gap {
  first: string;
  kind: GapKind;
  last: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Record".
 */
export interface Record {
  data: unknown;
  kind: string;
  operation_id?: string;
  run_id?: string;
  sequence: string;
  session_id: string;
  time_ms: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Interaction".
 */
export interface Interaction {
  answer?: unknown;
  id: string;
  request: InteractionCreate;
  revision: string;
  session_id: string;
  status: InteractionStatus;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Run".
 */
export interface Run {
  command_id: string;
  completion?: RecordReference;
  error?: ErrorResponse;
  id: string;
  session_id: string;
  status: RunStatus;
  step: number;
  wait?: Wait;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Schedule".
 */
export interface Schedule {
  due_ms: number;
  id: string;
  request: ScheduleCreate;
  session_id: string;
  status: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ScheduleCreate".
 */
export interface ScheduleCreate {
  due_ms: number;
  input: InputData;
  interval_ms?: number;
  request_id: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "SessionView".
 */
export interface SessionView {
  config: SessionConfig;
  created_ms: number;
  id: string;
  lifecycle: Lifecycle;
  owner_epoch: string;
  parent?: string;
  paused: boolean;
  revision: string;
  sequence: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Handoff".
 */
export interface Handoff {
  harness: HarnessSelection;
  request_id: string;
  revision: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "HarnessRequest".
 */
export interface HarnessRequest {
  checkpoint: unknown;
  children?: ChildResult[];
  command_id: string;
  config: SessionConfig;
  context: Record[];
  input: InputData;
  outcome?: Operation;
  protocol: string;
  run_id: string;
  session_id: string;
  step: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Input".
 */
export interface Input {
  data: InputData;
  request_id: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "InteractionAnswer".
 */
export interface InteractionAnswer {
  accept: boolean;
  answer: unknown;
  request_id: string;
  revision: string;
  subject_digest: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ManagedInvocation".
 */
export interface ManagedInvocation {
  callback_url: string;
  execution: Execution;
  package: Package;
  secrets: {
    [k: string]: string | undefined;
  };
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Package".
 */
export interface Package {
  digest: string;
  error?: string;
  id: string;
  image?: string;
  manifest: PackageManifest;
  status: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "PackageManifest".
 */
export interface PackageManifest {
  actions: Action[];
  cpu_millis: number;
  entrypoint: string;
  files: {
    [k: string]: string | undefined;
  };
  language: RuntimeLanguage;
  lockfile: string;
  memory_bytes: number;
  name: string;
  network: string[];
  secrets: string[];
  version: string;
  wall_ms: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ModelMessage".
 */
export interface ModelMessage {
  content: unknown;
  role: string;
  tool_call_id?: string;
  tool_calls?: ModelToolCall[];
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ModelToolCall".
 */
export interface ModelToolCall {
  arguments: unknown;
  id: string;
  name: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ModelRequest".
 */
export interface ModelRequest {
  actions: Action[];
  messages: ModelMessage[];
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ModelResponse".
 */
export interface ModelResponse {
  message: ModelMessage;
  native?: {
    [k: string]: unknown | undefined;
  };
  usage: Usage;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "NodeInfo".
 */
export interface NodeInfo {
  draining: boolean;
  endpoint: string;
  id: string;
  lease_expires_ms: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "PackageAdmission".
 */
export interface PackageAdmission {
  builder: string;
  manifest: PackageManifest;
  request_id: string;
  session_id: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "PackageBuildResult".
 */
export interface PackageBuildResult {
  base_image: string;
  builder: string;
  image: string;
  package_id: string;
  source_digest: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ParticipantGrant".
 */
export interface ParticipantGrant {
  expires_ms: number;
  permissions: string[];
  request_id: string;
  subject: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "ParticipantToken".
 */
export interface ParticipantToken {
  expires_ms: number;
  id: string;
  token: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Receipt".
 */
export interface Receipt {
  error?: string;
  outcome: Outcome;
  request_id: string;
  result?: unknown;
  usage?: Usage;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Retry".
 */
export interface Retry {
  acknowledge_uncertainty: boolean;
  request_id: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "SessionBudget".
 */
export interface SessionBudget {
  active_operations: number;
  available_bytes: number;
  available_operations: number;
  available_tokens?: string;
  calls: number;
  incomplete_usages: number;
  remaining_calls: number;
  reported_tokens: string;
  reserved_output_bytes: number;
  reserved_tokens: string;
  retained_bytes: number;
  session_id: string;
  token_usage_pending: boolean;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "SessionCreate".
 */
export interface SessionCreate {
  config: SessionConfig;
  message?: string;
  request_id: string;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "SessionPage".
 */
export interface SessionPage {
  next?: string;
  sessions: SessionView[];
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "StorageReport".
 */
export interface StorageReport {
  accepting: boolean;
  archive_queue_bytes: number;
  cache_bytes: number;
  format: string;
  oldest_unarchived_ms: number;
  reserved_output_bytes: number;
  retained_bytes: number;
  sessions: number;
  unarchived_bytes: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "Transfer".
 */
export interface Transfer {
  checkpoint?: RecordReference;
  error?: ErrorResponse;
  id: string;
  phase: TransferPhase;
  session_id: string;
  source_epoch: string;
  source_node: string;
  target_node: string;
  through: string;
  verified_runs: number;
}
/**
 * This interface was referenced by `BrainSessionAPIV2`'s JSON-Schema
 * via the `definition` "TransferCreate".
 */
export interface TransferCreate {
  owner_epoch: string;
  request_id: string;
  target_node: string;
}
