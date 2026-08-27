export { BrainClient as Brain, BrainClient, SessionHandle } from "./client.js";
export type { BrainOptions } from "./client.js";
export { defineAgentLoop, defineEnvironment, defineTool } from "./extensions.js";
export { BrainError } from "./errors.js";
export type {
  AgentLoop,
  BoundTool,
  CreateSessionOptions,
  Environment,
  EnvironmentLifecycle,
  OperationOptions,
  SessionEvent,
  SessionState,
  Tool,
  ToolBindingOptions,
  ToolDefinition,
  VercelAiGatewayModel,
} from "./types.js";
export { contractDigests } from "./generated/contracts.js";
