import type { z } from "zod";

declare const agentloopBrand: unique symbol;
declare const environmentBrand: unique symbol;
declare const boundToolBrand: unique symbol;
declare const clientToolBrand: unique symbol;
declare const servedToolBrand: unique symbol;

export type Schema = z.ZodType;
export type SchemaInput<Value extends Schema> = z.input<Value>;
export type SchemaOutput<Value extends Schema> = z.output<Value>;

export interface Agentloop { readonly [agentloopBrand]: true }
export interface Environment { readonly [environmentBrand]: true }
/** A built tool placed in the environment that executes it: the result of calling a
 * tool factory with `{ env, ...options }`. */
export interface BoundTool<Input = unknown, Output = unknown> {
  readonly [boundToolBrand]: { readonly input: Input; readonly output: Output };
}
/** A tool that runs in this process: declared with `tool({ ..., execute })` and
 * passed straight to `sessions.create` — no environment. The SDK answers its calls
 * off the session's event feed. */
export interface ClientTool<Input = unknown, Output = unknown> {
  readonly [clientToolBrand]: { readonly input: Input; readonly output: Output };
}
/** A tool declared without an `execute`: some other process answers it by joining
 * the session with its share key and calling `serve`. */
export interface ServedTool<Input = unknown, Output = unknown> {
  readonly [servedToolBrand]: { readonly input: Input; readonly output: Output };
}
/** Anything `sessions.create` accepts in `tools`. */
export type SessionTool = BoundTool | ClientTool | ServedTool;

export interface ToolDefinition {
  readonly name: string;
  readonly description: string;
  readonly inputSchema: Readonly<Record<string, unknown>>;
  readonly outputSchema?: Readonly<Record<string, unknown>>;
}

/** The closed set of program kinds an environment can launch — closed only because
 * Brain and the SDK must physically package and start the program. */
export type Runtime = "esm" | "shell" | "http";

/** A resource name: a lowercase word, optionally namespaced with one colon (`fs`,
 * `process`, `bin:ffmpeg`). The contract fixes the policy shape of the named
 * resources; vendor resources are opaque to Brain. */
export type ResourceName = string;

export interface FsResource { readonly root: string }
export interface ProcessResource { readonly timeout_ms_max?: number; readonly output_bytes_max?: number }
export interface NetResource { readonly allow: readonly string[] }
export type DomResource = Record<never, never>;
export interface SecretsResource { readonly names: readonly string[] }
/** What an environment declares a program will find there, keyed by resource name.
 * Brain compares the names against each bound tool's `needs` at session create and
 * never interprets the policy blocks; enforcement is the platform's, behind the
 * environment. */
export interface Resources {
  readonly fs?: FsResource;
  readonly process?: ProcessResource;
  readonly net?: NetResource;
  readonly dom?: DomResource;
  readonly secrets?: SecretsResource;
  readonly [vendor: `${string}:${string}`]: Readonly<Record<string, unknown>> | undefined;
}

/** The request template of an `http` program: the environment fronts the endpoint,
 * the tool's input travels as the JSON body, and the response body is the output. */
export interface HttpProgramRequest {
  readonly method: string;
  readonly url: string;
  readonly headers?: Readonly<Record<string, string>>;
}
/** The program behind a provisioned tool, named by content identity. An `esm`
 * bundle travels out of band under its identity; a `shell` script and an `http`
 * request template travel inline. */
export type Program =
  | { readonly kind: "esm"; readonly identity: string }
  | { readonly kind: "shell"; readonly identity: string; readonly script: string }
  | { readonly kind: "http"; readonly identity: string; readonly request: HttpProgramRequest };

/** The one envelope every tool invocation resolves to.
 * `timeout` is the caller-owned deadline firing — distinguished, never an exit
 * code. */
export type Outcome<Value = unknown> =
  | { readonly status: "ok"; readonly value: Value }
  | { readonly status: "error"; readonly error: { readonly code: string; readonly message: string; readonly details?: unknown } }
  | { readonly status: "timeout" }
  | { readonly status: "cancelled" };

export type { KnownProviderId } from "./generated/providers.js";
import type { KnownProviderId } from "./generated/providers.js";

export interface VercelAiGatewayModel {
  readonly provider: "vercel-ai-gateway";
  readonly name: `${string}/${string}`;
  readonly apiKey: string;
}
/** A provider from the generated models.dev catalog. */
export interface KnownProviderModel {
  readonly provider: Exclude<KnownProviderId, "vercel-ai-gateway">;
  readonly name: string;
  readonly apiKey: string;
}
/** Any other provider the server's deployment registers (a custom providers
 * file, a proxy). `string & {}` keeps autocomplete for the known ids while
 * still accepting an arbitrary identifier; unknown providers are rejected by
 * the server, not the client. */
export interface CustomProviderModel {
  readonly provider: string & {};
  readonly name: string;
  readonly apiKey: string;
}
export type ModelSelection = VercelAiGatewayModel | KnownProviderModel | CustomProviderModel;

/** The provider-neutral message model. History is authored once, in this
 * shape; Brain renders it per provider dialect at request build time. */
export type ModelContentBlock =
  | { readonly type: "text"; readonly text: string }
  | { readonly type: "tool_use"; readonly id: string; readonly name: string; readonly input: unknown }
  | { readonly type: "tool_result"; readonly tool_use_id: string; readonly content: unknown; readonly is_error: boolean };
export interface ModelMessage {
  readonly role: "user" | "assistant";
  readonly content: readonly ModelContentBlock[];
}
export type ModelStopReason = "end_turn" | "tool_use" | "max_tokens" | "stop_sequence" | "refusal" | "unknown";
/** Every field is optional because absent is never zero: a provider that did
 * not report a count did not report zero. */
export interface ModelUsage {
  readonly input_tokens?: number;
  readonly output_tokens?: number;
  readonly cache_read_input_tokens?: number;
  readonly cache_creation_input_tokens?: number;
  readonly reasoning_tokens?: number;
}
export interface ModelResponse {
  readonly message: ModelMessage;
  readonly stop_reason: ModelStopReason;
  readonly usage: ModelUsage;
}

/**
 * What `send` hands the session. The shape is closed on purpose: Brain owes every
 * agentloop the same observation shape regardless of who wrote the client. Multimodal
 * parts will extend this record when they land — see the roadmap.
 */
export interface UserInput { readonly message: string }

export interface CreateSessionOptions {
  readonly model: ModelSelection;
  readonly agentloop: Agentloop;
  readonly tools?: readonly SessionTool[];
  /**
   * Events from an earlier session, to carry a conversation forward.
   *
   * A session lives in the process that created it and does not survive a restart, so
   * keep the events you receive from `handle.events()` and pass them back here to
   * continue. Brain writes them as the new session's opening records and tells the
   * agentloop about them. Omit for an ordinary new session.
   */
  readonly history?: readonly SessionEvent[];
}

export interface OperationOptions { readonly idempotencyKey?: string }

export interface SessionState {
  readonly id: string;
  readonly status: "creating" | "idle" | "running" | "ended" | "failed";
  /** Sequence of the last journal record committed for this session — where an
   * events cursor starts. */
  readonly lastSequence: number;
  /** The scoped credential another process joins with to serve this session's
   * tools. It opens the serve feed and answers tool calls — nothing else — so it is
   * safe to hand to a page. */
  readonly shareKey: string;
}

export interface SessionEvent<Data = unknown> {
  readonly id: string;
  readonly sequence: number;
  readonly recordedAt: Date;
  readonly type: string;
  readonly data: Data;
}

/** One frame off the live event stream. Journalled records carry their sequence;
 * streaming deltas (`assistant_delta`, `tool_call_delta`, `refusal_delta`) are never
 * journalled and carry none. */
export interface SessionStreamEvent<Data = unknown> {
  readonly sequence?: number;
  readonly type: string;
  readonly data: Data;
}

export interface AgentloopAdmission {
  readonly identity: string;
  readonly status: "admitted" | "rejected";
  readonly error?: { readonly code: string; readonly message: string; readonly retryable: boolean; readonly details?: unknown };
}
