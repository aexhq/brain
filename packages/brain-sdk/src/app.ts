import { z } from "zod";

import { errorOutcome, MAX_DEADLINE_MS, parseCancelFrame, parseInvokeFrame, signatureHeader, verifySignature, type InvokeFrame, type Outcome } from "./callback-wire.js";
import type { CapabilityName, Schema, SchemaOutput } from "./types.js";

/** The contract of one app-hosted tool: what the model sees, declared where the
 * function lives. The function itself never leaves the app's process. */
export interface AppToolContract<InputSchema extends Schema = Schema, OutputSchema extends Schema | undefined = Schema | undefined> {
  readonly name: string;
  readonly description: string;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
}

export interface AppToolCall {
  readonly callId: string;
  /** When the environment's budget for this call runs out. */
  readonly deadline: Date;
  /** Fires on best-effort cancellation and when the deadline passes. */
  readonly signal: AbortSignal;
}

type AppToolHandler<Input, Output> = (input: Input, call: AppToolCall) => Output | Promise<Output>;

/** The `contracts/tool/v1` manifest of a callback tool: `hosting: "callback"`, no
 * payload — the code stays home. */
export interface CallbackToolManifest {
  readonly name: string;
  readonly description: string;
  readonly input_schema: Readonly<Record<string, unknown>>;
  readonly output_schema?: Readonly<Record<string, unknown>>;
  readonly requires: readonly CapabilityName[];
  readonly binding_names: readonly string[];
  readonly hosting: "callback";
}

/** The one registration surface, identical in both transport directions. */
export interface AppTools {
  register<InputSchema extends Schema, OutputSchema extends Schema | undefined = undefined>(
    contract: AppToolContract<InputSchema, OutputSchema>,
    handler: AppToolHandler<SchemaOutput<InputSchema>, OutputSchema extends Schema ? SchemaOutput<OutputSchema> : unknown>,
  ): this;
  manifests(): readonly CallbackToolManifest[];
}

/** Backend direction: the app listens and the environment POSTs signed invocations. */
export interface AppToolServer extends AppTools {
  fetchHandler(): (request: Request) => Promise<Response>;
}

/** Browser/outbound direction: the app holds a channel out to the environment. */
export interface AppToolChannel extends AppTools {
  /** Resolves when the channel is connected; a new promise after every drop. */
  ready(): Promise<void>;
  close(): void;
}

interface RegisteredAppTool {
  readonly contract: AppToolContract;
  readonly handler: AppToolHandler<unknown, unknown>;
}

/** Shared execution semantics for app-held tools, whichever transport delivers the
 * frame: schema-checked input and output, a clamped deadline race, best-effort
 * cancellation, exactly one Outcome. Internal to the SDK's transports. */
export class AppToolRegistry {
  private readonly tools = new Map<string, RegisteredAppTool>();
  private readonly active = new Map<string, { readonly controller: AbortController; cancelled: boolean }>();

  register(contract: AppToolContract, handler: AppToolHandler<unknown, unknown>): void {
    if (typeof contract?.name !== "string" || !/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(contract.name)) throw new TypeError("app tool name must be an identifier");
    if (typeof contract.description !== "string" || contract.description.length === 0 || contract.description.length > 8_192) throw new TypeError("app tool description exceeds its contract bound");
    if (typeof handler !== "function") throw new TypeError("app tool needs a handler function");
    if (this.tools.has(contract.name)) throw new TypeError(`app tool ${contract.name} is already registered`);
    this.tools.set(contract.name, { contract, handler });
  }

  manifests(): readonly CallbackToolManifest[] {
    return [...this.tools.values()].map(({ contract }) => Object.freeze({
      name: contract.name,
      description: contract.description,
      input_schema: Object.freeze(z.toJSONSchema(contract.input) as Record<string, unknown>),
      ...(contract.output === undefined ? {} : { output_schema: Object.freeze(z.toJSONSchema(contract.output) as Record<string, unknown>) }),
      requires: Object.freeze([]) as readonly CapabilityName[],
      binding_names: Object.freeze([]) as readonly string[],
      hosting: "callback" as const,
    }));
  }

  cancel(callId: string): void {
    const call = this.active.get(callId);
    if (call === undefined) return;
    call.cancelled = true;
    call.controller.abort(new Error("call cancelled"));
  }

  async run(frame: InvokeFrame): Promise<Outcome> {
    const registered = this.tools.get(frame.name);
    if (registered === undefined) return errorOutcome("unknown_tool", `no app tool named ${frame.name} is registered`);
    let input: unknown;
    try {
      input = registered.contract.input.parse(frame.arguments);
    } catch (error) {
      return errorOutcome("invalid_input", String(error instanceof Error ? error.message : error));
    }
    const call = { controller: new AbortController(), cancelled: false };
    this.active.set(frame.call_id, call);
    const deadlineMs = frame.deadline_ms > MAX_DEADLINE_MS ? MAX_DEADLINE_MS : frame.deadline_ms;
    const timer = setTimeout(() => call.controller.abort(new Error("call deadline passed")), deadlineMs);
    const interrupted = new Promise<typeof interruption>((resolve) => call.controller.signal.addEventListener("abort", () => resolve(interruption), { once: true }));
    try {
      const value = await Promise.race([
        Promise.resolve(registered.handler(input, { callId: frame.call_id, deadline: new Date(Date.now() + deadlineMs), signal: call.controller.signal })),
        interrupted,
      ]);
      if (value === interruption) return { status: call.cancelled ? "cancelled" : "timeout" };
      if (registered.contract.output === undefined) return { status: "ok", value: value ?? null };
      try {
        return { status: "ok", value: registered.contract.output.parse(value) ?? null };
      } catch (error) {
        return errorOutcome("invalid_output", String(error instanceof Error ? error.message : error));
      }
    } catch (error) {
      if (call.controller.signal.aborted) return { status: call.cancelled ? "cancelled" : "timeout" };
      return errorOutcome("tool_error", String(error instanceof Error ? error.message : error));
    } finally {
      clearTimeout(timer);
      this.active.delete(frame.call_id);
    }
  }
}

const interruption = Symbol("call interrupted");

function requireOption(value: string | undefined, name: string): string {
  if (typeof value !== "string" || value.length === 0) throw new TypeError(`appTools needs a non-empty ${name}`);
  return value;
}

/**
 * Host callback tools in a backend app: mount the returned fetch handler and the
 * environment POSTs HMAC-signed invocations to it. Unsigned or malformed requests are
 * refused before any tool code runs; a handler that throws becomes an error outcome.
 */
export function appTools(options: { readonly signingKey: string | undefined }): AppToolServer {
  const signingKey = requireOption(options?.signingKey, "signingKey");
  const registry = new AppToolRegistry();
  const server: AppToolServer = {
    register(contract, handler) {
      registry.register(contract, handler as AppToolHandler<unknown, unknown>);
      return server;
    },
    manifests: () => registry.manifests(),
    fetchHandler() {
      return async (request: Request): Promise<Response> => {
        if (request.method !== "POST") return failure(405, "method_not_allowed", "callback invocations are POSTed");
        const body = await request.text();
        const signature = request.headers.get(signatureHeader);
        if (signature === null || !verifySignature(body, signature, signingKey)) return failure(401, "invalid_signature", "the request is not signed with this app's key");
        let frame: unknown;
        try {
          frame = JSON.parse(body);
        } catch {
          return failure(400, "invalid_request", "the request body is not JSON");
        }
        const cancel = parseCancelFrame(frame);
        if (cancel !== undefined) {
          registry.cancel(cancel.cancel);
          return Response.json({ status: "ok", value: null });
        }
        const invoke = parseInvokeFrame(frame);
        if (invoke === undefined) return failure(400, "invalid_request", "the request is not an invocation frame");
        return Response.json(await registry.run(invoke));
      };
    },
  };
  return Object.freeze(server);
}

function failure(status: number, code: string, message: string): Response {
  return Response.json({ code, message, retryable: false }, { status });
}

/**
 * Host callback tools from a process that cannot listen (a browser page, a laptop
 * behind NAT): hold an outbound WebSocket to the environment's channel endpoint,
 * answer invocation frames with outcomes, honor best-effort cancel, and reconnect
 * with backoff. While the socket is down the environment answers invocations with a
 * typed "app disconnected" error — nothing hangs here or there.
 */
appTools.connect = function connect(options: { readonly url: string | URL; readonly token: string | undefined }): AppToolChannel {
  const token = requireOption(options?.token, "token");
  const target = new URL(options.url);
  target.searchParams.set("token", token);
  const registry = new AppToolRegistry();
  let socket: WebSocket | undefined;
  let closed = false;
  let attempt = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let readyResolvers: (() => void)[] = [];

  const open = (): void => {
    if (closed) return;
    const ws = new WebSocket(target);
    socket = ws;
    ws.addEventListener("open", () => {
      attempt = 0;
      const resolvers = readyResolvers;
      readyResolvers = [];
      for (const resolve of resolvers) resolve();
    });
    ws.addEventListener("message", (event) => { void answer(ws, event.data); });
    ws.addEventListener("error", () => {});
    ws.addEventListener("close", () => {
      if (socket !== ws) return;
      socket = undefined;
      if (closed) return;
      timer = setTimeout(open, Math.min(10_000, 250 * 2 ** attempt));
      attempt += 1;
      (timer as { unref?(): void }).unref?.();
    });
  };

  const answer = async (ws: WebSocket, data: unknown): Promise<void> => {
    let frame: unknown;
    try {
      const text = typeof data === "string" ? data
        : data instanceof Blob ? await data.text()
        : new TextDecoder().decode(data as ArrayBuffer);
      frame = JSON.parse(text);
    } catch {
      return;
    }
    const cancel = parseCancelFrame(frame);
    if (cancel !== undefined) {
      registry.cancel(cancel.cancel);
      return;
    }
    const invoke = parseInvokeFrame(frame);
    if (invoke === undefined) return;
    const outcome = await registry.run(invoke);
    if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ call_id: invoke.call_id, ...outcome }));
  };

  open();
  const channel: AppToolChannel = {
    register(contract, handler) {
      registry.register(contract, handler as AppToolHandler<unknown, unknown>);
      return channel;
    },
    manifests: () => registry.manifests(),
    ready(): Promise<void> {
      if (closed) return Promise.reject(new Error("the channel is closed"));
      if (socket !== undefined && socket.readyState === WebSocket.OPEN) return Promise.resolve();
      return new Promise((resolve) => readyResolvers.push(resolve));
    },
    close(): void {
      closed = true;
      if (timer !== undefined) clearTimeout(timer);
      socket?.close();
      socket = undefined;
      readyResolvers = [];
    },
  };
  return Object.freeze(channel);
};
