import { createInterface } from "node:readline";
import { mkdir, rm } from "node:fs/promises";
import { join } from "node:path";
import type { Readable, Writable } from "node:stream";
import { z } from "zod";
import { EnvironmentServices } from "./environments.js";
import { environmentDefinitions } from "./generated/environment-schema.js";
import type {
  EventPage, HarnessInitialize, HarnessRun, HarnessSession as SessionRequest,
  Message, ModelRequest, ModelResult, ToolInvocation, ToolReturn, TurnInput,
} from "./generated/session.js";

export const HARNESS_PROTOCOL = "brain/harness/v1";

export interface HarnessContext extends Omit<TurnInput, "kv"> {
  readonly sessionId: string;
  readonly sequence: number;
  readonly modelInfo: HarnessRun["model"];
  readonly signal: AbortSignal;
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

export interface HarnessSession {
  run(context: HarnessContext): unknown | Promise<unknown>;
  cancel?(): void | Promise<void>;
  /** Release resources; leave native state available for open(). */
  close(): void | Promise<void>;
}

export interface AgentHarness {
  readonly name: string;
  readonly stateVersion: string;
  initialize?(options: HarnessInitialize): void | Promise<void>;
  open(options: { sessionId: string; stateDirectory: string; configuration: unknown }): HarnessSession | Promise<HarnessSession>;
  /** Delete any externally stored native state. Brain also removes stateDirectory. */
  delete?(options: { sessionId: string; stateDirectory: string }): void | Promise<void>;
  shutdown?(): void | Promise<void>;
}

/** A failed Brain callback. Fatal failures stop the activation even if upstream catches them. */
export class HarnessServiceError extends Error {
  constructor(message: string, readonly code: string, readonly fatal: boolean) { super(message); }
}

export function createHarnessContext(
  run: HarnessRun, signal: AbortSignal,
  request: (method: string, input: unknown) => Promise<unknown>,
): HarnessContext {
  const callTools = (calls: ToolInvocation[]) => request("dispatch", calls) as Promise<ToolReturn[]>;
  return {
    ...run.input, sessionId: run.session_id, sequence: run.sequence, modelInfo: run.model, signal,
    kv: {
      get: async <T>(key: string) => (await request("kv_read", key) as { value?: T }).value,
      set: (key, value) => request("kv_put", { key, value }) as Promise<number>,
      delete: key => request("kv_delete", key) as Promise<number>,
    },
    environments: new EnvironmentServices(input => request("environments", input)),
    model: input => request("model", input) as Promise<ModelResult>,
    callTool: async call => (await callTools([call]))[0]!, callTools,
    setTranscript: messages => request("set_transcript", messages) as Promise<number>,
    readEvents: after => request("events", after) as Promise<EventPage>,
    emit: (kind, data) => request("emit", { event_type: kind, data }) as Promise<number>,
    telemetry: async record => { await request("telemetry", record); },
  };
}

const schemas = new Map<string, z.ZodType>();
function parse<T>(name: string, input: unknown): T {
  let schema = schemas.get(name);
  if (schema === undefined) {
    schema = z.fromJSONSchema({ $defs: environmentDefinitions, $ref: `#/$defs/${name}` });
    schemas.set(name, schema);
  }
  return schema.parse(input) as T;
}

/** Serve the bidirectional stdio protocol. stdout is reserved for JSON-RPC. */
export async function serveHarness(
  harness: AgentHarness,
  { input = process.stdin, output = process.stdout }: { input?: Readable; output?: Writable } = {},
): Promise<void> {
  let initialized: HarnessInitialize | undefined;
  let initializing = false;
  let next = 1;
  let stopped = false;
  const sessions = new Map<string, Promise<HarnessSession>>();
  const runs = new Map<string, { controller: AbortController; done: Promise<unknown> }>();
  const pending = new Map<string, { resolve(value: unknown): void; reject(error: Error): void }>();
  const requests = new Set<Promise<void>>();
  let writes = Promise.resolve();
  const write = (value: unknown) => {
    writes = writes.then(() => new Promise<void>((resolve, reject) => {
      output.write(`${JSON.stringify(value)}\n`, error => error ? reject(error) : resolve());
    }));
    return writes;
  };
  async function call(run: HarnessRun, method: string, value: unknown): Promise<unknown> {
    const id = `brain:${next++}`;
    const response = new Promise<unknown>((resolve, reject) => pending.set(id, { resolve, reject }));
    try {
      await write({ jsonrpc: "2.0", id, method: "brain/call", params: {
        session_id: run.session_id, sequence: run.sequence, method, input: value,
      } });
      return await response;
    } finally { pending.delete(id); }
  }
  async function close(id: string): Promise<void> {
    const active = runs.get(id);
    if (active !== undefined) {
      active.controller.abort(new Error("harness session closed"));
      await (await sessions.get(id))?.cancel?.();
      await active.done.catch(() => undefined);
    }
    const session = await sessions.get(id);
    if (session !== undefined) { await session.close(); sessions.delete(id); }
  }
  async function handle(method: string, raw: unknown): Promise<unknown> {
    if (method === "initialize") {
      if (initialized !== undefined || initializing) throw new Error("harness already initialized");
      initializing = true;
      const options = parse<HarnessInitialize>("HarnessInitialize", raw);
      if (options.protocol !== HARNESS_PROTOCOL) throw new Error("unsupported harness protocol");
      await harness.initialize?.(options);
      initialized = options;
      return { protocol: HARNESS_PROTOCOL, name: harness.name, state_version: harness.stateVersion };
    }
    if (initialized === undefined || stopped) throw new Error("harness is not running");
    if (method === "shutdown") {
      stopped = true;
      await Promise.all([...sessions.keys()].map(close));
      await harness.shutdown?.();
      return null;
    }
    if (method === "session/run") {
      const run = parse<HarnessRun>("HarnessRun", raw);
      if (runs.has(run.session_id)) throw new Error("harness session is busy");
      const opening = sessions.get(run.session_id);
      if (opening === undefined) throw new Error("harness session is not open");
      const controller = new AbortController();
      const callbacks = new Set<Promise<unknown>>();
      let fatal: unknown;
      let active = true;
      const done = Promise.resolve().then(async () => {
        const session = await opening;
        const context = createHarnessContext(run, controller.signal, (method, value) => {
          if (!active) return Promise.reject(new Error("harness activation has ended"));
          if (fatal !== undefined) return Promise.reject(fatal);
          controller.signal.throwIfAborted();
          const callback = call(run, method, value).catch(error => {
            if (error instanceof HarnessServiceError && error.fatal) {
              fatal = error;
              controller.abort(error);
              // Do not await cancel inside a callback: native abort may wait for this very call.
              void Promise.resolve(session.cancel?.()).catch(() => undefined);
            }
            throw error;
          });
          callbacks.add(callback);
          void callback.finally(() => callbacks.delete(callback)).catch(() => undefined);
          return callback;
        });
        let result: unknown;
        try { result = await session.run(context); }
        finally { active = false; await Promise.allSettled(callbacks); }
        if (fatal !== undefined) throw fatal;
        controller.signal.throwIfAborted();
        return result === undefined ? {} : { result };
      });
      runs.set(run.session_id, { controller, done });
      try { return await done; } finally { runs.delete(run.session_id); }
    }
    const { session_id: id } = parse<SessionRequest>("HarnessSession", raw);
    switch (method) {
      case "session/open": {
        if (sessions.has(id)) throw new Error("harness session is already open");
        const stateDirectory = join(initialized.state_directory, id);
        const opening = Promise.resolve().then(async () => {
          await mkdir(stateDirectory, { recursive: true });
          return harness.open({ sessionId: id, stateDirectory, configuration: initialized!.configuration });
        });
        sessions.set(id, opening);
        try { await opening; } catch (error) { sessions.delete(id); throw error; }
        return null;
      }
      case "session/cancel": {
        runs.get(id)?.controller.abort(new Error("harness activation cancelled"));
        await (await sessions.get(id))?.cancel?.();
        return null;
      }
      case "session/close": await close(id); return null;
      case "session/delete": {
        await close(id);
        const stateDirectory = join(initialized.state_directory, id);
        await harness.delete?.({ sessionId: id, stateDirectory });
        await rm(stateDirectory, { recursive: true, force: true });
        return null;
      }
      default: throw new Error(`unknown harness method: ${method}`);
    }
  }
  try {
    for await (const line of createInterface({ input, crlfDelay: Infinity })) {
      const frame = JSON.parse(line);
      if (frame?.jsonrpc !== "2.0" || !(typeof frame.id === "string" || typeof frame.id === "number")) throw new Error("invalid JSON-RPC frame");
      if (typeof frame.method !== "string") {
        const reply = pending.get(frame.id);
        if (reply === undefined) throw new Error("unknown Brain callback id");
        pending.delete(frame.id);
        if (frame.error !== undefined) reply.reject(new HarnessServiceError(frame.error.message, frame.error.data?.code ?? "callback_failed", frame.error.data?.fatal === true));
        else if (Object.hasOwn(frame, "result")) reply.resolve(frame.result);
        else throw new Error("missing Brain callback result");
        continue;
      }
      const request = Promise.resolve().then(async () => {
        try { await write({ jsonrpc: "2.0", id: frame.id, result: await handle(frame.method, frame.params) }); }
        catch (error) {
          await write({ jsonrpc: "2.0", id: frame.id, error: { code: -32000, message: error instanceof Error ? error.message : String(error) } });
        }
      });
      requests.add(request);
      void request.finally(() => requests.delete(request)).catch(() => undefined);
    }
  } finally {
    stopped = true;
    for (const reply of pending.values()) reply.reject(new HarnessServiceError("Brain transport closed", "ambiguous", true));
    pending.clear();
    await Promise.allSettled([...sessions.keys()].map(close));
    await Promise.allSettled(requests);
    await writes;
  }
}

export { environmentDefinitions as harnessDefinitions } from "./generated/environment-schema.js";
export type { ActivationTool, ContentBlock, Media, Message, ModelResult, Usage, ToolResult, ToolReturn } from "./generated/session.js";
