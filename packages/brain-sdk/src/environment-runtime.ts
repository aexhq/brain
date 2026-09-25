import { z } from "zod";
import { BrainError } from "./errors.js";
import { EnvironmentServices } from "./environments.js";
import type { EnvironmentCommand, EnvironmentEvent, EnvironmentMethod, EnvironmentObservation, EnvironmentReceipt, EnvironmentRef, EnvironmentRequest, EnvironmentResponse, ExecutionCallback } from "./generated/session.js";
import type { Schema, SchemaOutput } from "./types.js";

export interface EnvironmentReporter {
  emit(kind: string, data: unknown): Promise<number>;
  emitResult(observation: EnvironmentObservation, options?: { readonly content?: string }): Promise<number>;
}

export interface EnvironmentRunContext<Options> extends EnvironmentReporter {
  readonly sessionId: string;
  readonly sequence: number;
  readonly environment: EnvironmentRef;
  readonly options: Readonly<Options>;
  readonly signal: AbortSignal;
  readonly environments: EnvironmentServices;
  /** May be retained after this operation ends; has no operation services. */
  readonly reporter: EnvironmentReporter;
}

export interface EnvironmentMethodContract<Options, Input extends Schema = Schema, Output extends Schema | undefined = Schema | undefined> {
  readonly description: string;
  readonly input: Input;
  readonly output?: Output;
  readonly effect?: "none" | "replace";
  run(input: SchemaOutput<Input>, context: EnvironmentRunContext<Options>): unknown | Promise<unknown>;
}

type Options<S extends Schema | undefined> = S extends Schema ? SchemaOutput<S> : Record<string, never>;

export interface EnvironmentRuntimeContract<S extends Schema | undefined = undefined, Methods extends Record<string, Schema> = Record<string, Schema>> {
  readonly options?: S;
  readonly methods?: { readonly [K in keyof Methods]: EnvironmentMethodContract<Options<S>, Methods[K]> };
  setup?(context: EnvironmentRunContext<Options<S>>): void | { readonly onTurnEnd?: string } | Promise<void | { readonly onTurnEnd?: string }>;
  execute?(request: Extract<EnvironmentRequest, { type: "execute" }>, context: EnvironmentRunContext<Options<S>>): EnvironmentReceipt | Promise<EnvironmentReceipt>;
  cancel?(sequence: number, context: EnvironmentRunContext<Options<S>>): void | Promise<void>;
  detach?(context: EnvironmentRunContext<Options<S>>): void | Promise<void>;
  teardown?(context: EnvironmentRunContext<Options<S>>): void | Promise<void>;
}

/** The controller authenticates its HTTP caller before handing an operation to this handler. */
export function environmentHandler<S extends Schema | undefined = undefined, Methods extends Record<string, Schema> = Record<string, Schema>>(contract: EnvironmentRuntimeContract<S, Methods>) {
  const methods: Record<string, EnvironmentMethod> = Object.fromEntries(Object.entries(contract.methods ?? {}).map(([name, method]) => [name, {
    description: method.description,
    input_schema: z.toJSONSchema(method.input, { io: "input" }),
    ...(method.output === undefined ? {} : { output_schema: z.toJSONSchema(method.output) }),
    effect: method.effect ?? "none",
  }]));
  return {
    methods,
    async run(command: EnvironmentCommand, signal?: AbortSignal): Promise<EnvironmentResponse> {
      if (command.contract !== "environment/v1") throw new TypeError("unsupported Environment contract");
      const operation = command.operation;
      const lifetime = new AbortController();
      const active = signal === undefined ? lifetime.signal : AbortSignal.any([lifetime.signal, signal]);
      const call = (method: string, input: unknown) => {
        active.throwIfAborted();
        if (!operation.context?.methods.includes(method)) throw new BrainError(403, "not_granted", "Environment operation service is not granted", false);
        return post(operation.context, { method, input }, active);
      };
      const report = async (event: EnvironmentEvent): Promise<number> => {
        if (operation.reporter === undefined) throw new BrainError(403, "not_granted", "Environment reporter is not granted", false);
        const result = await post(operation.reporter, event) as { sequence: number };
        return result.sequence;
      };
      let receipt: EnvironmentReceipt;
      try {
        if (operation.binding === undefined) throw new TypeError("Environment operation requires a binding reference");
        const context: EnvironmentRunContext<Options<S>> = {
          sessionId: operation.session_id, sequence: operation.sequence, environment: operation.binding,
          options: (contract.options === undefined ? z.strictObject({}).parse(operation.configuration ?? {}) : contract.options.parse(operation.configuration)) as Options<S>, signal: active,
          environments: new EnvironmentServices(request => call("environments", request)),
          emit: async (event_type, data) => await call("emit", { event_type, data }) as number,
          emitResult: async (observation, options = {}) => await call("result", { observation, ...options }) as number,
          reporter: {
            emit: (event_type, data) => report({ type: "event", event_type, data }),
            emitResult: (observation, options = {}) => report({ type: "result", output: { observation, ...options } }),
          },
        };
        const request = operation.request;
        switch (request.type) {
          case "setup": {
            const result = await contract.setup?.(context);
            receipt = { type: "accepted", ...(result?.onTurnEnd === undefined ? {} : { on_turn_end: result.onTurnEnd }) };
            break;
          }
          case "call": {
            const method = contract.methods?.[request.name];
            if (method === undefined) throw new BrainError(400, "unsupported", "Environment method is not declared", false);
            const output = await method.run(method.input.parse(request.input), context);
            receipt = { type: "result", output: method.output === undefined ? output : method.output.parse(output) };
            break;
          }
          case "execute":
            if (contract.execute === undefined) throw new BrainError(400, "unsupported", "Environment does not execute implementations", false);
            receipt = await contract.execute(request, context);
            break;
          case "cancel":
            if (contract.cancel === undefined) throw new BrainError(400, "unsupported", "Environment does not cancel execution", false);
            await contract.cancel(request.target_sequence, context);
            receipt = { type: "accepted" };
            break;
          case "detach":
          case "teardown":
            await contract[request.type]?.(context);
            receipt = { type: "accepted" };
            break;
        }
      } catch (error) {
        receipt = error instanceof BrainError && ["ambiguous", "unknown"].includes(error.code)
          ? { type: "unknown", message: error.message }
          : { type: "failure", code: error instanceof BrainError ? error.code : "environment_error",
              message: error instanceof Error ? error.message : String(error), retryable: error instanceof BrainError && error.retryable,
              ...(error instanceof BrainError && error.details !== undefined ? { details: error.details } : {}) };
      } finally {
        lifetime.abort();
      }
      return { contract: "environment/v1", sequence: operation.sequence, receipt };
    },
  };
}

async function post(callback: ExecutionCallback, input: unknown, signal?: AbortSignal): Promise<unknown> {
  const response = await fetch(callback.url, { method: "POST", headers: { authorization: `Bearer ${callback.token}`, "content-type": "application/json" }, body: JSON.stringify(input), ...(signal === undefined ? {} : { signal }) });
  const body = await response.json();
  if (!response.ok) throw new BrainError(response.status, body.code, body.message, body.retryable, body.details);
  return body;
}
