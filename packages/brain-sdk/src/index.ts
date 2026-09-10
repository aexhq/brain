export { BrainClient as Brain, BrainClient, SessionHandle, Sessions } from "./client.js";
export type { BrainOptions, HostCredentials } from "./client.js";
export {
  agentloop,
  brainEnv,
  component,
  environment,
  hostEnv,
  inspectAgentloop,
  inspectComponent,
  inspectEnvironment,
  inspectTool,
  tool,
} from "./extensions.js";
export type {
  AgentloopContract,
  BrainEnvOptions,
  EnvironmentContract,
  EnvironmentDriver,
  ToolContract,
  ToolRunContext,
} from "./extensions.js";
export type { HostToolCall } from "./host.js";
export { BrainError } from "./errors.js";
export { StructuredOutputError } from "./structured-output.js";
export type { EventPage, ModelRequest, ModelResult, SessionTranscript, ToolResult } from "./generated/session.js";
export type {
  AgentloopAdmission,
  Component,
  CreateSessionOptions,
  CustomProviderModel,
  Environment,
  KnownProviderId,
  KnownProviderModel,
  ModelContentBlock,
  ModelMessage,
  ModelResponse,
  ModelSelection,
  ModelStopReason,
  ModelUsage,
  OperationOptions,
  SendOptions,
  StructuredSendOptions,
  Media,
  UserInput,
  Outcome,
  PlacedAgentloop,
  PlacedTool,
  Schema,
  SchemaInput,
  SchemaOutput,
  SessionEvent,
  SessionState,
  SessionStreamEvent,
  SessionTool,
  ToolDefinition,
  VercelAiGatewayModel,
} from "./types.js";
