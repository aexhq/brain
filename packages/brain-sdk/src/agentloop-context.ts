import { EnvironmentServices } from "./environments.js";
import type { EventPage, Message, ModelRequest, ModelResult, ToolInvocation, ToolReturn, TurnInput } from "./generated/session.js";

export interface AgentloopContext extends Omit<TurnInput, "kv"> {
  readonly kv: {
    get<T = unknown>(key: string): Promise<T | undefined>;
    set(key: string, value: unknown): Promise<number>;
    delete(key: string): Promise<number>;
  };
  readonly environments: EnvironmentServices;
  model(request: ModelRequest): Promise<ModelResult>;
  callTool(call: ToolInvocation): Promise<ToolReturn>;
  callTools(calls: ToolInvocation[]): Promise<ToolReturn[]>;
  setTranscript(messages: Message[]): Promise<number>;
  readEvents(after: number): Promise<EventPage>;
  emit(kind: string, data: unknown): Promise<number>;
  telemetry(record: unknown): Promise<void>;
}

export type AgentloopHandler = (ctx: AgentloopContext) => unknown | Promise<unknown>;

/** Runtime adapters supply an invocation-scoped transport. */
export function createAgentloopContext(
  input: TurnInput,
  request: (method: string, input: unknown) => Promise<unknown>,
): AgentloopContext {
  const callTools = (calls: ToolInvocation[]) => request("dispatch", calls) as Promise<ToolReturn[]>;
  return {
    ...input,
    kv: {
      get: async <T>(key: string) => (await request("kv_read", key) as { value?: T }).value,
      set: (key, value) => request("kv_put", { key, value }) as Promise<number>,
      delete: key => request("kv_delete", key) as Promise<number>,
    },
    environments: new EnvironmentServices(input => request("environments", input)),
    model: input => request("model", input) as Promise<ModelResult>,
    callTool: async call => (await callTools([call]))[0]!,
    callTools,
    setTranscript: messages => request("set_transcript", messages) as Promise<number>,
    readEvents: after => request("events", after) as Promise<EventPage>,
    emit: (kind, data) => request("emit", { event_type: kind, data }) as Promise<number>,
    telemetry: async record => { await request("telemetry", record); },
  };
}
