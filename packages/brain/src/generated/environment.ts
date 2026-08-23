/* eslint-disable */
/** GENERATED from Brain-owned contracts/session/v1. DO NOT EDIT. */

/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "Digest".
 */
export type Digest = string;
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "Identifier".
 */
export type Identifier = string;
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "ArtifactTarget".
 */
export type ArtifactTarget = "linux-amd64" | "linux-arm64";
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "TargetKind".
 */
export type TargetKind = "environment" | "additional";
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "EnvironmentCapability".
 */
export type EnvironmentCapability =
  "execution" | "session_preparation" | "sandbox_files" | "sandbox_control";
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "RecoveryClass".
 */
export type RecoveryClass = "retained" | "connection_scoped";
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "NetworkCeiling".
 */
export type NetworkCeiling =
  | {
      kind: "none";
    }
  | {
      kind: "public";
    }
  | {
      kind: "allowlist";
      /**
       * @minItems 1
       * @maxItems 128
       */
      destinations: [
        (
          | {
              host: string;
              /**
               * @minItems 1
               * @maxItems 1
               */
              ports: [unknown];
              protocol: "tls";
            }
          | {
              cidr: string;
              /**
               * @minItems 1
               * @maxItems 32
               */
              ports: [number, ...number[]];
              protocol: "tcp";
            }
        ),
        ...(
          | {
              host: string;
              /**
               * @minItems 1
               * @maxItems 1
               */
              ports: [unknown];
              protocol: "tls";
            }
          | {
              cidr: string;
              /**
               * @minItems 1
               * @maxItems 32
               */
              ports: [number, ...number[]];
              protocol: "tcp";
            }
        )[]
      ];
    };
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "OperationState".
 */
export type OperationState = "accepted" | "running" | "terminal" | "unknown";
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "TerminalOutcome".
 */
export type TerminalOutcome =
  "completed" | "failed" | "cancelled" | "deadline_exceeded" | "interrupted";
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SandboxState".
 */
export type SandboxState =
  "never_materialized" | "creating" | "running" | "suspended" | "gone" | "terminated";
/**
 * Inline content is standard padded base64 and capped at 1 MiB decoded. Larger writes carry an opaque object identity plus a one-purpose GET authority.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SandboxFileWriteSource".
 */
export type SandboxFileWriteSource =
  | {
      kind: "inline";
      content_base64: string;
    }
  | {
      kind: "object";
      object: ObjectReference;
      fetch: ObjectTransferAuthority;
    };
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "EnvironmentErrorCode".
 */
export type EnvironmentErrorCode =
  | "binding_conflict"
  | "capability_unavailable"
  | "operation_conflict"
  | "operation_unknown"
  | "sandbox_not_materialized"
  | "sandbox_gone"
  | "file_not_found"
  | "generation_conflict"
  | "invalid_request"
  | "resource_exhausted"
  | "temporarily_unavailable";

/**
 * The single current, transport-neutral Brain to Environment receipt contract. The canonical schema digest is the compatibility identity; the wire carries no protocol version.
 */
export interface BrainEnvironmentContract {
  contract: {
    methods: ["resolve_binding", "submit", "observe", "cancel", "acknowledge_terminal"];
  };
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "ObjectReference".
 */
export interface ObjectReference {
  object_id: Identifier;
  bytes: number;
  sha256: Digest;
  media_type?: string | null;
}
/**
 * Short-lived, one-purpose transfer capability minted by Brain-owned storage. transfer_id identifies the reservation/capability; object_id is the immutable source or pending destination identity. GET is valid only for import and PUT only for export; Environments never infer an object-store key. Export returns ObjectReference.object_id exactly equal to this sealed object_id.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "ObjectTransferAuthority".
 */
export interface ObjectTransferAuthority {
  transfer_id: Identifier;
  object_id: Identifier;
  method: "GET" | "PUT";
  url: string;
  headers: {
    [k: string]: string | undefined;
  };
  expires_at_ms: number;
  max_bytes: number;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "ArtifactLayerDescriptor".
 */
export interface ArtifactLayerDescriptor {
  digest: Digest;
  bytes: number;
  media_type: "application/javascript+esm" | "application/x-xz";
  mount_path: string;
  unpack: "file" | "tar.xz";
  object: ObjectReference;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "BundleDescriptor".
 */
export interface BundleDescriptor {
  bundle_digest: Digest;
  bytes: number;
  target: ArtifactTarget;
  execute_path: string;
  setup_path?: string | null;
  /**
   * @minItems 1
   * @maxItems 16
   */
  layers:
    | [ArtifactLayerDescriptor]
    | [ArtifactLayerDescriptor, ArtifactLayerDescriptor]
    | [ArtifactLayerDescriptor, ArtifactLayerDescriptor, ArtifactLayerDescriptor]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ]
    | [
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor,
        ArtifactLayerDescriptor
      ];
  tool_name: Identifier;
  environment_name: Identifier;
  description?: string | null;
  contract_digest: Digest;
  /**
   * @maxItems 128
   */
  required_env: Identifier[];
}
/**
 * Short-lived, one-purpose fetch authority supplied only at preparation time; it is not part of the persisted sealed binding.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "BundleFetch".
 */
export interface BundleFetch {
  bundle_digest: Digest;
  url: string;
  headers: {
    [k: string]: string | undefined;
  };
  expires_at_ms: number;
  max_bytes: number;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "PreparedBindingBundles".
 */
export interface PreparedBindingBundles {
  binding_ref: Identifier;
  bundle_digests: Digest[];
}
/**
 * Opaque, short-lived, one-redemption authority for one session and one physical target generation. The Environment may keep redeemed values only in supervisor memory and inject each binding's declared subset at child spawn. Brain may mint a replacement capability for the same surviving generation after a Environment control-process crash. Secret values never enter this contract, binding registry, journal, receipt or argv.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SecretCapability".
 */
export interface SecretCapability {
  capability_ref: Identifier;
  expires_at_ms: number;
  /**
   * @maxItems 128
   */
  env_names: Identifier[];
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SecretDeliveryRequest".
 */
export interface SecretDeliveryRequest {
  capability_ref: Identifier;
  environment_id: Identifier;
  session_id: Identifier;
  root_id: Identifier;
  target: SandboxTarget;
  generation_intent: Identifier;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SandboxTarget".
 */
export interface SandboxTarget {
  kind: TargetKind;
  session_id: Identifier;
  root_id: Identifier;
  binding_ref: Identifier;
  sandbox_id?: Identifier | null;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "EnvironmentProfile".
 */
export interface EnvironmentProfile {
  kind: "computer" | "callbacks";
  platform?: "linux-amd64" | "linux-arm64";
  network: "none" | "allowlist" | "unrestricted";
  recovery: "retained" | "connection" | "replay_safe";
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "ResolvedBinding".
 */
export interface ResolvedBinding {
  binding_ref: Identifier;
  environment_id: Identifier;
  recovery: RecoveryClass;
  capabilities: EnvironmentCapability[];
  limits: {
    max_inline_input_bytes: number;
    max_inline_result_bytes: number;
    max_wait_ms: number;
  };
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SealedBinding".
 */
export interface SealedBinding {
  binding_id: Identifier;
  session_id: Identifier;
  root_id: Identifier;
  contract_digest: Digest;
  implementation_identity: Digest;
  extension: string;
  protocol: "environment/v1";
  profile: EnvironmentProfile;
  configuration: {
    [k: string]: unknown | undefined;
  };
  environment_name: Identifier;
  capability: Identifier;
  policy_digest: Digest;
  bundle?: BundleDescriptor | null;
  required_capabilities?: EnvironmentCapability[];
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "ResourceCeiling".
 */
export interface ResourceCeiling {
  timeout_ms: number;
  max_output_bytes: number;
}
/**
 * Canonical JSON Tool arguments only. Brain rejects serialized input above 192 KiB before submit. Large data is referenced by storage key, URL, or sandbox path and transferred through typed streaming authorities, never embedded as a managed Tool argument.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "OperationInput".
 */
export interface OperationInput {
  kind: "inline";
  value: unknown;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "OperationEnvelope".
 */
export interface OperationEnvelope {
  operation_id: Identifier;
  request_digest: Digest;
  session_id: Identifier;
  root_id: Identifier;
  turn_id: Identifier;
  caller_id: Identifier;
  fence: number;
  generation?: string | null;
  binding_ref: Identifier;
  capability: Identifier;
  input: OperationInput;
  phase: "setup" | "execute";
  target_ref?: string | null;
  deadline_at_ms: number;
  resources: ResourceCeiling;
  network: NetworkCeiling;
  trace: {
    [k: string]: string | undefined;
  };
}
/**
 * Durable execution locator carrying both the opaque receipt and the exact rooted target authority required to observe, cancel, acknowledge, and reconcile target loss.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "OperationRef".
 */
export interface OperationRef {
  operation_id: Identifier;
  request_digest: Digest;
  target: SandboxTarget1;
  generation: Identifier;
  /**
   * Opaque physical target locator paired with generation. It never replaces the rooted logical target.
   */
  target_ref: string;
  /**
   * Opaque Environment-issued locator for the accepted physical execution. Brain journals it before observe/cancel/ack; it complements the Environment binding/preparation/target registry and never encodes product routing policy.
   */
  receipt_ref: string;
}
/**
 * Exact rooted logical target accepted for this execution. Control and acknowledgement calls carry it back so Environment can reconcile its root-keyed target registry without a reverse index or scan.
 */
export interface SandboxTarget1 {
  kind: TargetKind;
  session_id: Identifier;
  root_id: Identifier;
  binding_ref: Identifier;
  sandbox_id?: Identifier | null;
}
/**
 * Environment-issued continuity locator for a materialized target. Brain journals and projects the newest receipt, then supplies target_ref and generation on later operations.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "TargetReceipt".
 */
export interface TargetReceipt {
  target_ref: Identifier;
  generation: Identifier;
  expires_at_ms: number;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "TerminalResult".
 */
export interface TerminalResult {
  outcome: TerminalOutcome;
  terminal_digest: Digest;
  is_error: boolean;
  /**
   * Inline JSON result. Its RFC 8785 encoding must be at most 94208 bytes; larger data is returned by object/storage key/path.
   */
  inline?: {
    [k: string]: unknown | undefined;
  };
  object?: ObjectReference | null;
  exit_code?: number | null;
  duration_ms?: number;
}
/**
 * One bounded output observation emitted by a Environment. Brain treats it as provisional until the terminal receipt is durably journaled.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "OutputChunk".
 */
export interface OutputChunk {
  stream: "stdout" | "stderr" | "progress";
  offset: number;
  text: string;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "OperationObservation".
 */
export interface OperationObservation {
  operation: OperationRef;
  state: OperationState;
  output: OutputChunk[];
  next_cursor: string;
  target?: TargetReceipt | null;
  terminal?: TerminalResult | null;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SubmitRequest".
 */
export interface SubmitRequest {
  envelope: OperationEnvelope;
  wait_up_to_ms: number;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SubmitReceipt".
 */
export interface SubmitReceipt {
  operation: OperationRef;
  replayed: boolean;
  observation: OperationObservation;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "ObserveRequest".
 */
export interface ObserveRequest {
  operation: OperationRef;
  cursor: string;
  wait_ms: number;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "CancelRequest".
 */
export interface CancelRequest {
  operation: OperationRef;
  reason: string;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "CancellationReceipt".
 */
export interface CancellationReceipt {
  operation: OperationRef;
  accepted: boolean;
  observation: OperationObservation;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "AcknowledgeTerminalRequest".
 */
export interface AcknowledgeTerminalRequest {
  operation: OperationRef;
  terminal_digest: Digest;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "Acknowledgement".
 */
export interface Acknowledgement {
  acknowledged: boolean;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SandboxStatus".
 */
export interface SandboxStatus {
  target: SandboxTarget;
  state: SandboxState;
  target_ref?: Identifier | null;
  generation?: string | null;
  reason?: string | null;
  changed_at_ms?: number | null;
  expires_at_ms: number | null;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "FileEntry".
 */
export interface FileEntry {
  path: string;
  kind: "file" | "directory" | "symlink";
  bytes: number;
  sha256?: Digest | null;
  modified_at_ms: number;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SandboxFileRequest".
 */
export interface SandboxFileRequest {
  target: SandboxTarget;
  expected_generation: Identifier;
  path: string;
}
/**
 * Effect identity is exact across ambiguous transport delivery. Environment retains and replays the byte-equivalent result for the same operation_id and request_digest until the target is purged; a different digest conflicts before effect.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SandboxFileWriteRequest".
 */
export interface SandboxFileWriteRequest {
  operation_id: Identifier;
  request_digest: Digest;
  target: SandboxTarget;
  expected_generation: Identifier;
  path: string;
  source: SandboxFileWriteSource;
  overwrite: boolean;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SandboxFileWriteResult".
 */
export interface SandboxFileWriteResult {
  operation_id: Identifier;
  request_digest: Digest;
  replayed: boolean;
  file: FileEntry;
}
/**
 * Effect identity is exact across ambiguous transport delivery. Environment retains and replays the byte-equivalent result for the same operation_id and request_digest until the target is purged; a different digest conflicts before effect.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SandboxCopyRequest".
 */
export interface SandboxCopyRequest {
  operation_id: Identifier;
  request_digest: Digest;
  target: SandboxTarget;
  expected_generation: Identifier;
  path: string;
  object: ObjectReference | null;
  transfer: ObjectTransferAuthority;
  direction: "import" | "export";
  overwrite: boolean;
}
/**
 * Import returns object=null. Export returns the uploaded object identity so Brain can verify and durably publish it.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SandboxCopyResult".
 */
export interface SandboxCopyResult {
  operation_id: Identifier;
  request_digest: Digest;
  replayed: boolean;
  file: FileEntry;
  object: ObjectReference | null;
}
/**
 * Execute with /bin/bash -lc in the selected additional sandbox. Environment secrets are never accepted from model input; declared server-tool env is delivered through SecretCapability.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SandboxExecInput".
 */
export interface SandboxExecInput {
  command: string;
  cwd?: string | null;
  interactive: boolean;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "SandboxExecutionRequest".
 */
export interface SandboxExecutionRequest {
  target: SandboxTarget;
  expected_generation: Identifier;
  execution_id: Identifier;
  request_digest: Digest;
  input: SandboxExecInput;
  resources: ResourceCeiling;
  network: NetworkCeiling;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "PrepareSessionRequest".
 */
export interface PrepareSessionRequest {
  session_id: Identifier;
  root_id: Identifier;
  bindings: PreparedBindingBundles[];
  bundles: BundleFetch[];
  network: NetworkCeiling;
  resources: ResourceCeiling;
  secret_capability?: SecretCapability | null;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "PreparedSession".
 */
export interface PreparedSession {
  preparation_ref: Identifier;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "CreateSandboxRequest".
 */
export interface CreateSandboxRequest {
  target: SandboxTarget;
  generation_intent: Identifier;
  resource_class: Identifier;
  resources: ResourceCeiling;
  network: NetworkCeiling;
}
/**
 * One idempotent stdin append/EOF/poll. Empty text with eof=false is a pure poll. UTF-8 payload bytes are additionally capped at 4096 so the Environment can perform one PIPE_BUF-bounded write; larger input must be split into separately identified requests.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "WriteStdinRequest".
 */
export interface WriteStdinRequest {
  operation_id: Identifier;
  request_digest: Digest;
  target: SandboxTarget;
  expected_generation: Identifier;
  execution_id: Identifier;
  text: string;
  eof: boolean;
}
/**
 * Exact stdin-effect receipt plus the current bounded observation of the referenced interactive execution. Poll requests return accepted=false and still provide the observation.
 *
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "WriteStdinReceipt".
 */
export interface WriteStdinReceipt {
  operation_id: Identifier;
  request_digest: Digest;
  accepted: boolean;
  replayed: boolean;
  observation: OperationObservation;
}
/**
 * This interface was referenced by `BrainEnvironmentContract`'s JSON-Schema
 * via the `definition` "EnvironmentError".
 */
export interface EnvironmentError {
  code: EnvironmentErrorCode;
  message: string;
  retryable: boolean;
  details?: {
    [k: string]: unknown | undefined;
  };
}
