import type { AgentLoop, BoundTool, Environment, EnvironmentLifecycle, Tool, ToolBindingOptions, ToolDefinition } from "./types.js";

export function defineAgentLoop(packageArtifact: URL | Uint8Array): AgentLoop {
  if (!(packageArtifact instanceof URL) && !(packageArtifact instanceof Uint8Array)) {
    throw new TypeError("AgentLoop package must be a URL or Uint8Array");
  }
  return Object.freeze({ kind: "agent-loop", package: packageArtifact });
}

export function defineEnvironment<Capability extends string>(options: {
  capability: Capability;
  configuration: unknown;
  lifecycle?: EnvironmentLifecycle;
}): Environment<Capability> {
  if (typeof options.capability !== "string" || options.capability.trim() === "") {
    throw new TypeError("Environment capability must be a non-empty string");
  }
  const lifecycle = options.lifecycle ?? {};
  validateLifecycle(lifecycle);
  return Object.freeze({
    kind: "environment",
    capability: options.capability,
    configuration: structuredClone(options.configuration),
    lifecycle: Object.freeze({ ...lifecycle }),
  });
}

export function defineTool<Input = unknown, Output = unknown, CompatibleEnvironment extends Environment = Environment>(options: {
  environmentCapability: CompatibleEnvironment["capability"];
  definition: ToolDefinition;
  remoteToolId?: string;
  defaultGrant?: unknown;
}): Tool<Input, Output, CompatibleEnvironment> {
  validateTool(options.definition, options.remoteToolId ?? options.definition.name);
  const definition = Object.freeze({
    ...options.definition,
    inputSchema: Object.freeze(structuredClone(options.definition.inputSchema)),
    ...(options.definition.outputSchema === undefined
      ? {}
      : { outputSchema: Object.freeze(structuredClone(options.definition.outputSchema)) }),
  });
  const remoteToolId = options.remoteToolId ?? definition.name;
  const defaultGrant = structuredClone(options.defaultGrant ?? {});
  return Object.freeze({
    kind: "tool" as const,
    environmentCapability: options.environmentCapability,
    definition,
    remoteToolId,
    defaultGrant,
    runIn(environment: CompatibleEnvironment, binding: ToolBindingOptions = {}): BoundTool<Input, Output> {
      if (environment.kind !== "environment") throw new TypeError("runIn requires an Environment");
      if (environment.capability !== options.environmentCapability) {
        throw new TypeError(`Tool ${definition.name} cannot run in a ${environment.capability} Environment`);
      }
      return Object.freeze({
        kind: "bound-tool" as const,
        tool: this,
        environment,
        grant: structuredClone(binding.grant ?? defaultGrant),
      });
    },
  });
}

function validateLifecycle(lifecycle: EnvironmentLifecycle): void {
  const type = lifecycle.type ?? "session";
  if (type === "session") {
    if (lifecycle.id !== undefined) throw new TypeError("a session Environment cannot declare an id");
    return;
  }
  if ((type !== "shared" && type !== "external") || !validIdentifier(lifecycle.id)) {
    throw new TypeError("a shared or external Environment requires a stable id");
  }
}

function validateTool(definition: ToolDefinition, remoteToolId: string): void {
  if (!validIdentifier(definition.name) || !validIdentifier(remoteToolId)) {
    throw new TypeError("Tool names must be stable identifiers");
  }
  if (typeof definition.description !== "string" || definition.description.length > 8_192) {
    throw new TypeError("Tool description exceeds its contract bound");
  }
  if (!plainObject(definition.inputSchema) || (definition.outputSchema !== undefined && !plainObject(definition.outputSchema))) {
    throw new TypeError("Tool schemas must be objects");
  }
}

function validIdentifier(value: unknown): value is string {
  return typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(value);
}

function plainObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
