import { Sessions } from "./session.js";
import type { Fetch } from "./transport.js";
import { Transport } from "./transport.js";

export {
  AbortError,
  BrainError,
  OutputRefusalError,
  OutputSchemaError,
  OutputValidationError,
  SessionError,
} from "./errors.js";
export type { BrainErrorOptions } from "./errors.js";
export { Session, Sessions } from "./session.js";
export type {
  CreateSessionOptions,
  ListSessionsOptions,
  ModelOptions,
  ModelSummary,
  OutputOptions,
  RequestOptions,
  SessionInput,
  SessionList,
  SessionSummary,
} from "./session.js";
export type { EventOptions } from "./transport.js";
export {
  MAX_SESSION_BUNDLE_BYTES,
  MAX_TOOL_BUNDLE_BYTES,
  compileTools,
  defineIntrinsicTool,
  definePreinstalledTool,
  defineServerTool,
  defineTool,
} from "./tools.js";
export type {
  CompiledTools,
  DefineIntrinsicToolOptions,
  DefinePreinstalledToolOptions,
  DefineServerToolOptions,
  DefineToolOptions,
  JsonSchema,
  Tool,
  ToolContext,
  ToolDefinition,
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

const DEFAULT_BRAIN_URL = "http://127.0.0.1:3210";

export interface BrainOptions {
  /** The standalone operator token or a downstream service bearer token. */
  token: string;
  baseUrl?: string;
  fetch?: Fetch;
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
    );
  }
}
