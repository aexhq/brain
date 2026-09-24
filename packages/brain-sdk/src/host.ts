import { z } from "zod";
import type { Outcome, Schema } from "./types.js";
import type { ModelRequest, ModelResult, ToolExecutionUpdate } from "./generated/session.js";

/** The model-facing contract lives with the function held by this process. */
export interface HostToolContract<InputSchema extends Schema = Schema, OutputSchema extends Schema | undefined = Schema | undefined> {
  readonly name: string;
  readonly description: string;
  readonly input: InputSchema;
  readonly output?: OutputSchema;
}

export interface ToolResultOptions {
  /** Text representing this result in the model conversation. */
  readonly content?: string;
}

export interface HostToolCall<Output = unknown> {
  readonly sessionId: string;
  /** The sequence of the tool_call_started record. */
  readonly sequence: number;
  /** The original expiry; absent when the call has no deadline. */
  readonly deadline: Date | undefined;
  readonly signal: AbortSignal;
  /** Append a durable extension Event. */
  emit(kind: string, data: unknown): Promise<number>;
  /** Append a result without completing this execution. */
  emitResult(value: Output | Outcome<Output>, options?: ToolResultOptions): Promise<number>;
  /** Commit completion, optionally with a final result. Use return call.finish(value). */
  finish(value?: Output | Outcome<Output>, options?: ToolResultOptions): Promise<void>;
  /** An independent call using the session model; does not edit conversation state. */
  model(request: ModelRequest): Promise<ModelResult>;
}

export type HostToolHandler<Input, Output> = (input: Input, call: HostToolCall) => Output | Outcome<Output> | void | Promise<Output | Outcome<Output> | void>;

export interface InvokeFrame {
  readonly sessionId: string;
  readonly environment: string;
  readonly sequence: number;
  readonly name: string;
  readonly arguments: unknown;
  readonly deadline_at_ms?: number;
  emit(kind: string, data: unknown): Promise<number>;
  update(value: ToolExecutionUpdate): Promise<number>;
  model(request: ModelRequest): Promise<ModelResult>;
}

const MAX_TIMER_MS = 2_147_483_647;

export function errorOutcome(code: string, message: string): Outcome {
  return { status: "error", error: { code, message: message.slice(0, 4096) } };
}

interface RegisteredHostTool {
  readonly contract: HostToolContract;
  readonly handler: HostToolHandler<unknown, unknown>;
}

type Interruption = Extract<Outcome, { status: "timeout" | "cancelled" | "unknown" }>;

/** A handler's return releases dispatch. Only explicit finish or interruption
 * closes the retained execution and its event authority. */
export class HostToolRegistry {
  private readonly tools = new Map<string, RegisteredHostTool>();
  private readonly active = new Map<number, (outcome: Interruption) => void>();

  register(environment: string, contract: HostToolContract, handler: HostToolHandler<unknown, unknown>): void {
    if (typeof contract?.name !== "string" || !/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(contract.name)) throw new TypeError("host tool name must be an identifier");
    if (typeof contract.description !== "string" || contract.description.length === 0 || contract.description.length > 8_192) throw new TypeError("host tool description exceeds its contract bound");
    if (typeof handler !== "function") throw new TypeError("host tool needs a handler function");
    const key = `${contract.name}\0${environment}`;
    if (this.tools.has(key)) throw new TypeError(`host tool ${contract.name} is already registered`);
    this.tools.set(key, { contract, handler });
  }

  cancel(sequence: number): void {
    this.active.get(sequence)?.({ status: "cancelled" });
  }

  disconnect(sequence: number): void {
    this.active.get(sequence)?.({ status: "unknown", message: "host connection was lost after dispatch" });
  }

  async run(frame: InvokeFrame): Promise<void> {
    const terminal = async (outcome: Outcome): Promise<void> => { await frame.update({ type: "finish", outcome }); };
    const registered = this.tools.get(`${frame.name}\0${frame.environment}`);
    if (registered === undefined) return terminal(errorOutcome("unknown_tool", `no host tool named ${frame.name} is registered`));
    const controller = new AbortController();
    const interrupted = new Promise<void>(resolve => controller.signal.addEventListener("abort", () => resolve(), { once: true }));
    const parse = (schema: Schema, value: unknown): Promise<unknown> => Promise.race([
      schema.parseAsync(value), interrupted.then(() => { throw controller.signal.reason; }),
    ]);
    let resolve!: () => void;
    let reject!: (reason: unknown) => void;
    const completed = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
    let finishing = false;
    let closing = false;
    let queue = Promise.resolve();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const enqueue = <T>(operation: () => Promise<T>): Promise<T> => {
      const pending = queue.then(operation);
      queue = pending.then(() => {}, () => {});
      return pending;
    };
    const ensureOpen = (): void => {
      if (closing || finishing) throw new Error("Tool execution is finished");
    };
    const commitFinish = async (outcome?: Outcome & ToolResultOptions): Promise<void> => {
      try {
        await frame.update({ type: "finish", ...(outcome === undefined ? {} : { outcome }) });
        resolve();
      } catch (error) { reject(error); throw error; }
    };
    const finish = (outcome?: Outcome & ToolResultOptions): Promise<void> => {
      if (finishing) throw new Error("Tool execution is finished");
      finishing = true;
      closing = true;
      clearTimeout(timer);
      return enqueue(() => commitFinish(outcome));
    };
    const normalize = async (value: unknown): Promise<Outcome> => {
      const status = typeof value === "object" && value !== null && "status" in value ? value.status : undefined;
      let outcome: Outcome = typeof status === "string" && ["ok", "error", "timeout", "cancelled", "unknown"].includes(status)
        ? outcomeSchema.parse(value)
        : { status: "ok", value };
      if (outcome.status === "ok" && registered.contract.output !== undefined) {
        outcome = { status: "ok", value: await parse(registered.contract.output, outcome.value) };
      }
      return outcomeSchema.parse(outcome);
    };
    const present = (outcome: Outcome, options?: ToolResultOptions): Outcome & ToolResultOptions => {
      if (options?.content === undefined) return outcome;
      if (typeof options.content !== "string") throw new TypeError("Tool content must be text");
      return { ...outcome, content: options.content };
    };
    const invalidOutput = async (error: unknown): Promise<never> => {
      if (!finishing) await finish(errorOutcome("invalid_output", message(error)));
      throw error;
    };
    this.active.set(frame.sequence, (outcome) => {
      if (finishing) return;
      controller.abort(new Error(outcome.status === "unknown" ? outcome.message : "host Tool " + outcome.status));
      void finish(outcome).catch(reject);
    });
    const schedule = (): void => {
      if (frame.deadline_at_ms === undefined || finishing) return;
      const remaining = frame.deadline_at_ms - Date.now();
      if (remaining <= 0) this.active.get(frame.sequence)?.({ status: "timeout" });
      else timer = setTimeout(schedule, Math.min(remaining, MAX_TIMER_MS));
    };
    schedule();
    try {
      if (!finishing) {
        void (async () => {
          try {
            let input: unknown;
            try { input = await parse(registered.contract.input, frame.arguments); }
            catch (error) {
              if (!finishing) await finish(errorOutcome("invalid_input", message(error)));
              return;
            }
            if (finishing) return;
            const value = await registered.handler(input, {
              sessionId: frame.sessionId,
              sequence: frame.sequence,
              deadline: frame.deadline_at_ms === undefined ? undefined : new Date(frame.deadline_at_ms),
              signal: controller.signal,
              emit: (kind, data) => {
                ensureOpen();
                return enqueue(async () => {
                  if (finishing) throw new Error("Tool execution is finished");
                  return frame.emit(kind, data);
                });
              },
              model: (request) => { ensureOpen(); return frame.model(request); },
              emitResult: async (value, options) => {
                ensureOpen();
                return enqueue(async () => {
                  const outcome = present(await normalize(value), options);
                  if (finishing) throw new Error("Tool execution is finished");
                  return frame.update({ type: "result", outcome });
                }).catch(invalidOutput);
              },
              finish: async (value, options) => {
                ensureOpen();
                closing = true;
                return enqueue(async () => {
                  const outcome = value === undefined && options?.content === undefined ? undefined : present(await normalize(value ?? null), options);
                  if (finishing) throw new Error("Tool execution is finished");
                  finishing = true;
                  clearTimeout(timer);
                  await commitFinish(outcome);
                }).catch(invalidOutput);
              },
            });
            if (closing || finishing) return;
            await enqueue(async () => {
              const outcome = value === undefined ? undefined : await normalize(value);
              if (closing || finishing) return;
              if (outcome !== undefined && outcome.status !== "ok") {
                finishing = true;
                closing = true;
                clearTimeout(timer);
                await commitFinish(outcome);
              } else await frame.update({ type: "returned", ...(outcome === undefined ? {} : { outcome }) });
            }).catch(invalidOutput);
          } catch (error) {
            if (!finishing) await finish(errorOutcome("tool_error", message(error)));
          }
        })().catch(reject);
      }
      await completed;
    } finally {
      clearTimeout(timer);
      this.active.delete(frame.sequence);
    }
  }
}

function message(error: unknown): string {
  return String(error instanceof Error ? error.message : error);
}

const outcomeSchema = z.discriminatedUnion("status", [
  z.strictObject({ status: z.literal("ok"), value: z.json() }),
  z.strictObject({ status: z.literal("error"), error: z.strictObject({
    code: z.string().regex(/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u),
    message: z.string().max(4096),
    retryable: z.boolean().optional(),
    details: z.json().optional(),
  }) }),
  z.strictObject({ status: z.literal("timeout") }),
  z.strictObject({ status: z.literal("cancelled") }),
  z.strictObject({ status: z.literal("unknown"), message: z.string() }),
]);
