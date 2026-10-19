import type { CreateSession, ModelCredential, ToolBinding } from "./generated.js";
import type { RuntimeTool } from "./runtime.js";

export const ApiKey = (value: string): ModelCredential => ({ api_key: value });
export const CredentialReference = (value: string): ModelCredential => ({ reference: value });
export const Model = (name: string, credential?: ModelCredential) => ({ name, credential });
export const Tool = (tool: RuntimeTool): RuntimeTool => tool;
export type Tool = import("./generated.js").Tool;

export interface ManagedSelection {
  kind: "managed";
  name: string;
  alias?: string;
  operations?: string[];
  parameters?: unknown;
  credential?: unknown;
}
export const ManagedTool = (name: string, options: Omit<ManagedSelection, "kind" | "name"> = {}): ManagedSelection => ({ kind: "managed", name, ...options });

export interface SessionOptions {
  harness: string;
  model: ReturnType<typeof Model>;
  tools?: (ManagedSelection | RuntimeTool)[];
  instructions?: string;
  reasoning_effort?: string;
}

export function lower(input: SessionOptions): { wire: CreateSession; runtime: RuntimeTool[] } {
  const names: string[] = [], bindings: ToolBinding[] = [], runtime: RuntimeTool[] = [];
  const credentials: NonNullable<CreateSession["credentials"]> = [];
  for (const tool of input.tools ?? []) {
    if (!("kind" in tool)) { runtime.push(tool); continue; }
    if (tool.operations === undefined) {
      if (tool.alias !== undefined || tool.parameters !== undefined || tool.credential !== undefined) throw new Error("provider tool options require operations");
      names.push(tool.name);
    } else {
      const alias = tool.alias ?? tool.name;
      bindings.push({ alias, provider: tool.name, operations: tool.operations, parameters: tool.parameters ?? {} });
      if (tool.credential !== undefined) credentials.push({ binding: alias, value: tool.credential });
    }
  }
  return { wire: {
    harness: input.harness,
    config: { model: input.model.name, tools: names, ...(bindings.length ? { bindings } : {}),
      ...(input.instructions === undefined ? {} : { instructions: input.instructions }),
      ...(input.reasoning_effort === undefined ? {} : { reasoning_effort: input.reasoning_effort }) },
    ...(input.model.credential === undefined ? {} : { model_credential: input.model.credential }),
    ...(credentials.length ? { credentials } : {}),
  }, runtime };
}
