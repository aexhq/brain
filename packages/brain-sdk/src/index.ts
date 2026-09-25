export { BrainClient as Brain, BrainClient, SessionHandle, Sessions } from "./client.js";
export type { BrainOptions, HostCredentials } from "./client.js";
export {
  agentloop,
  bindTool,
  brainEnv,
  component,
  environment,
  hostEnv,
  inspectAgentloop,
  inspectComponent,
  inspectEnvironment,
  inspectTool,
  tool,
  toolMetadata,
} from "./extensions.js";
export type {
  AgentloopContract,
  BrainEnvOptions,
  EnvironmentContract,
  EnvironmentDriver,
  ToolContract,
  ToolRunContext,
  ToolMetadata,
  ToolFactory,
  ToolPlacement,
} from "./extensions.js";
export type { HostToolCall, ToolResultOptions } from "./host.js";
export { BrainError } from "./errors.js";
export { StructuredOutputError } from "./structured-output.js";
export type { EventPage, ModelList, ModelProvider, ModelDef, ModelRequest, ModelResult, SessionTranscript, ToolResult } from "./generated/session.js";
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
  TurnOutcome,
  SessionStreamEvent,
  SessionTool,
  ToolDefinition,
  VercelAiGatewayModel,
} from "./types.js";
export { EnvironmentServices } from "./environments.js";
export { environmentHandler } from "./environment-runtime.js";
export type { EnvironmentRunContext, EnvironmentReporter, EnvironmentRuntimeContract, EnvironmentMethodContract } from "./environment-runtime.js";
export type { EnvironmentControlRequest, EnvironmentRef, EnvironmentView, EnvironmentGrant, EnvironmentLifecycle, EnvironmentMethod, EnvironmentObservation, EnvironmentOutput, EnvironmentTemplate } from "./environments.js";
export type { EnvironmentAvailability, EnvironmentState, EnvironmentPermission, EnvironmentMethodEffect, EnvironmentResolution, EnvironmentCommand, EnvironmentResponse, EnvironmentReceipt } from "./generated/session.js";
