import type { Outcome, Schema } from "./types.js";

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
  /** When Brain's budget for this call runs out. */
  readonly deadline: Date;
  /** Fires on best-effort cancellation and when the deadline passes. */
  readonly signal: AbortSignal;
  /** Append a durable extension Event to this session. */
  emit(kind: string, data: unknown): Promise<number>;
}

export type AppToolHandler<Input, Output> = (input: Input, call: AppToolCall) => Output | Promise<Output>;

/** One invocation as the pump hands it to the registry. `deadline_ms` is the
 * remaining budget, not an epoch. */
export interface InvokeFrame {
  readonly call_id: string;
  readonly name: string;
  readonly arguments: unknown;
  readonly deadline_ms: number;
  emit(kind: string, data: unknown): Promise<number>;
}

/** Ceiling on a wire-provided call deadline: generous next to Brain's default
 * tool deadline, small enough that a hostile frame cannot pin a timer for hours. */
export const MAX_DEADLINE_MS = 600_000;

export function errorOutcome(code: string, message: string): Outcome {
  return { status: "error", error: { code, message: message.slice(0, 4096) } };
}

interface RegisteredAppTool {
  readonly contract: AppToolContract;
  readonly handler: AppToolHandler<unknown, unknown>;
}

/** Shared execution semantics for app-held tools, whoever answers the session's
 * feed: schema-checked input and output, a clamped deadline race, best-effort
 * cancellation, exactly one Outcome. Internal to the SDK's pumps. */
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
        Promise.resolve(registered.handler(input, {
          callId: frame.call_id,
          deadline: new Date(Date.now() + deadlineMs),
          signal: call.controller.signal,
          emit: frame.emit,
        })),
        interrupted,
      ]);
      if (value === interruption) return { status: "unknown", message: call.cancelled ? "resident Tool was cancelled after dispatch" : "resident Tool exceeded its deadline after dispatch" };
      if (registered.contract.output === undefined) return { status: "ok", value: value ?? null };
      try {
        return { status: "ok", value: registered.contract.output.parse(value) ?? null };
      } catch (error) {
        return errorOutcome("invalid_output", String(error instanceof Error ? error.message : error));
      }
    } catch (error) {
      if (call.controller.signal.aborted) return { status: "unknown", message: call.cancelled ? "resident Tool was cancelled after dispatch" : "resident Tool exceeded its deadline after dispatch" };
      return errorOutcome("tool_error", String(error instanceof Error ? error.message : error));
    } finally {
      clearTimeout(timer);
      this.active.delete(frame.call_id);
    }
  }
}

const interruption = Symbol("call interrupted");
