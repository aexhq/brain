import { Sessions } from "./session.js";
import type { Fetch } from "./transport.js";
import { Transport } from "./transport.js";
import type { WebSocketFactory } from "./customer.js";

export {
  AbortError,
  BrainError,
  SessionError,
} from "./errors.js";
export type { BrainErrorOptions } from "./errors.js";
export { Session, Sessions } from "./session.js";
export type {
  CreateSessionOptions,
  ListSessionsOptions,
  ModelOptions,
  ModelSummary,
  ComponentToolConfig,
  NetworkDestination,
  NetworkPolicy,
  RequestOptions,
  SessionInput,
  SessionList,
  SessionSummary,
  SessionTool,
} from "./session.js";
export type { EventOptions } from "./transport.js";
export type { JsonRequestOptions, SessionTransport } from "./transport.js";
export {
  MAX_INLINE_FILE_BYTES,
  SandboxFiles,
  SessionChild,
  SessionChildren,
  SessionSandbox,
  SessionStorage,
} from "./resources.js";
export type {
  ChildList,
  SandboxFileEntry,
  SandboxFileList,
  SandboxStatus,
  StorageList,
  StorageObject,
  TransferTicket,
} from "./resources.js";
export {
  CustomerEnvironment,
  customerTerminalDigest,
} from "./customer.js";
export {
  MAX_CREATE_SESSION_REQUEST_BYTES,
  MAX_CUSTOMER_OBSERVATION_BYTES,
  MAX_CUSTOMER_REGISTRATIONS,
  MAX_CUSTOMER_REGISTRATION_DESCRIPTOR_BYTES,
  MAX_CUSTOMER_WS_FRAME_BYTES,
  MAX_EXTERNAL_TOOL_INPUT_BYTES,
  MAX_EXTERNAL_TOOL_REQUEST_BYTES,
  MAX_EXTERNAL_TOOL_RESPONSE_BYTES,
  MAX_MANAGED_TOOL_INPUT_BYTES,
  MAX_MESSAGE_REQUEST_BYTES,
  MAX_PUBLIC_EVENT_BYTES,
  MAX_TOOL_TERMINAL_INLINE_BYTES,
} from "./limits.js";
export type {
  CustomerEnvironmentChannel,
  CustomerEnvironmentConnector,
  CustomerEnvironmentOptions,
  CustomerObservation,
  WebSocketFactory,
  WebSocketRequest,
} from "./customer.js";
export {
  MAX_SESSION_BUNDLE_BYTES,
  MAX_TOOL_BUNDLE_BYTES,
  compileTools,
  tool,
} from "./tools.js";
export type {
  ClientRegistration,
  ClientToolOptions,
  CompiledTools,
  JsonSchema,
  ServerToolOptions,
  Tool,
  ToolBuilder,
  ToolContract,
  ToolContext,
  ToolExecution,
  ToolExecutor,
  ToolHandler,
  WireTool,
  WireToolBundle,
  WireToolDefinition,
  WireToolExecutor,
} from "./tools.js";
export { buildToolModule } from "./builder.js";
export type { PreparedBundle } from "./builder.js";
export type * as session from "./generated/session.js";
export * from "./generated/components.js";
export * from "./components.js";

const DEFAULT_BRAIN_URL = "http://127.0.0.1:3210";

export interface BrainOptions {
  /** The standalone operator token or a downstream service bearer token. */
  token: string;
  baseUrl?: string;
  fetch?: Fetch;
  webSocketFactory?: WebSocketFactory;
  /** Stable tenant-scoped identity of this customer application runner. */
  client?: { id: string };
}

/** A neutral client for one long-lived, multi-session Brain server. */
export class Brain {
  readonly sessions: Sessions;

  constructor(options: BrainOptions) {
    if (options.token.trim() === "") throw new TypeError("Brain token cannot be empty");
    const fetchImplementation = options.fetch ?? globalThis.fetch?.bind(globalThis);
    if (fetchImplementation === undefined) {
      throw new TypeError("This runtime does not provide fetch; pass a fetch implementation to Brain");
    }
    this.sessions = new Sessions(
      new Transport(options.token, options.baseUrl ?? DEFAULT_BRAIN_URL, fetchImplementation),
      options.webSocketFactory ??
        (globalThis.WebSocket === undefined
          ? undefined
          : (request) => new globalThis.WebSocket(request.url, request.protocol)),
      options.client?.id,
    );
  }

  /** Release process-scoped customer Tool sockets, heartbeats, and reconnect work. */
  close(): void {
    this.sessions.close();
  }
}
