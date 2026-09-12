import { z } from "zod";
import type { Outcome, Schema } from "./types.js";

/** The contract of one Tool this process holds: what the model sees, declared where
 * the function lives. The function itself never leaves the process. */
export interface HostToolContract<InputSchema extends Schema = Schema, OutputSchema extends Schema | undefined = Schema | undefined> {
  readonly name: string;
  readonly description: string;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
}

export interface HostToolCall {
  readonly sessionId: string;
  /** The sequence of the `tool_call_started` record: with the session id, the name of
   * this call everywhere. */
  readonly sequence: number;
  /** When Brain's budget for this call runs out. */
  readonly deadline: Date;
  /** Fires on best-effort cancellation and when the deadline passes. */
  readonly signal: AbortSignal;
  /** Append a durable extension Event to this session. */
  emit(kind: string, data: unknown): Promise<number>;
}

export type HostToolHandler<Input, Output> = (input: Input, call: HostToolCall) => Output | Outcome<Output> | Promise<Output | Outcome<Output>>;

/** One invocation as the pump hands it to the registry. `deadline_ms` is the
 * remaining budget, not an epoch. */
export interface InvokeFrame {
  readonly sessionId: string;
  readonly environment: string;
  readonly sequence: number;
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

interface RegisteredHostTool {
  readonly contract: HostToolContract;
  readonly handler: HostToolHandler<unknown, unknown>;
}

type Interruption = Extract<Outcome, { status: "timeout" | "cancelled" | "unknown" }>;

/** Shared execution semantics for the Tools this process holds, whoever answers the
 * session's feed: schema-checked input and output, a clamped deadline race, best-effort
 * cancellation, exactly one Outcome. Internal to the SDK's pump. */
export class HostToolRegistry {
  private readonly tools = new Map<string, RegisteredHostTool>();
  private readonly active = new Map<number, { readonly controller: AbortController; outcome?: Interruption }>();

  register(environment: string, contract: HostToolContract, handler: HostToolHandler<unknown, unknown>): void {
    if (typeof contract?.name !== "string" || !/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(contract.name)) throw new TypeError("host tool name must be an identifier");
    if (typeof contract.description !== "string" || contract.description.length === 0 || contract.description.length > 8_192) throw new TypeError("host tool description exceeds its contract bound");
    if (typeof handler !== "function") throw new TypeError("host tool needs a handler function");
    const key = `${contract.name}\0${environment}`;
    if (this.tools.has(key)) throw new TypeError(`host tool ${contract.name} is already registered`);
    this.tools.set(key, { contract, handler });
  }

  cancel(sequence: number): void {
    this.interrupt(sequence, { status: "cancelled" });
  }

  disconnect(sequence: number): void {
    this.interrupt(sequence, { status: "unknown", message: "host connection was lost after dispatch" });
  }

  private interrupt(sequence: number, outcome: Interruption): void {
    const call = this.active.get(sequence);
    if (call === undefined || call.outcome !== undefined) return;
    call.outcome = outcome;
    call.controller.abort(new Error(outcome.status === "unknown" ? outcome.message : "host Tool " + outcome.status));
  }

  async run(frame: InvokeFrame): Promise<Outcome> {
    const registered = this.tools.get(`${frame.name}\0${frame.environment}`);
    if (registered === undefined) return errorOutcome("unknown_tool", `no host tool named ${frame.name} is registered`);
    let input: unknown;
    try {
      input = registered.contract.input.parse(frame.arguments);
    } catch (error) {
      return errorOutcome("invalid_input", String(error instanceof Error ? error.message : error));
    }
    const call: { controller: AbortController; outcome?: Interruption } = { controller: new AbortController() };
    this.active.set(frame.sequence, call);
    const deadlineMs = frame.deadline_ms > MAX_DEADLINE_MS ? MAX_DEADLINE_MS : frame.deadline_ms;
    const timer = setTimeout(() => this.interrupt(frame.sequence, { status: "timeout" }), deadlineMs);
    const interrupted = new Promise<typeof interruption>((resolve) => call.controller.signal.addEventListener("abort", () => resolve(interruption), { once: true }));
    try {
      const value = await Promise.race([
        Promise.resolve(registered.handler(input, {
          sessionId: frame.sessionId,
          sequence: frame.sequence,
          deadline: new Date(Date.now() + deadlineMs),
          signal: call.controller.signal,
          emit: frame.emit,
        })),
        interrupted,
      ]);
      if (call.outcome !== undefined) return call.outcome;
      try {
        const status = typeof value === "object" && value !== null && "status" in value ? value.status : undefined;
        const outcome: Outcome = typeof status === "string" && ["ok", "error", "timeout", "cancelled", "unknown"].includes(status)
          ? outcomeSchema.parse(value)
          : { status: "ok", value: value ?? null };
        if (outcome.status !== "ok" || registered.contract.output === undefined) return outcome;
        return { status: "ok", value: registered.contract.output.parse(outcome.value) ?? null };
      } catch (error) {
        return errorOutcome("invalid_output", String(error instanceof Error ? error.message : error));
      }
    } catch (error) {
      if (call.outcome !== undefined) return call.outcome;
      return errorOutcome("tool_error", String(error instanceof Error ? error.message : error));
    } finally {
      clearTimeout(timer);
      this.active.delete(frame.sequence);
    }
  }
}

const interruption = Symbol("call interrupted");

const outcomeSchema = z.discriminatedUnion("status", [
  z.object({ status: z.literal("ok"), value: z.json() }),
  z.object({ status: z.literal("error"), error: z.strictObject({
    code: z.string().regex(/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u),
    message: z.string().max(4096),
    retryable: z.boolean().optional(),
    details: z.json().optional(),
  }) }),
  z.object({ status: z.literal("timeout") }),
  z.object({ status: z.literal("cancelled") }),
  z.object({ status: z.literal("unknown"), message: z.string() }),
]);
