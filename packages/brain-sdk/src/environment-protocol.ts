import { z } from "zod";
import { BrainError } from "./errors.js";
import { environmentDefinitions } from "./generated/environment-schema.js";
import type { EnvironmentCommand, EnvironmentOperation, EnvironmentReceipt, EnvironmentResponse } from "./generated/session.js";

function schema<T>(name: string): z.ZodType<T> {
  return z.fromJSONSchema({ $defs: environmentDefinitions, $ref: `#/$defs/${name}` }) as z.ZodType<T>;
}

let commandSchema: z.ZodType<EnvironmentCommand> | undefined;
let responseSchema: z.ZodType<EnvironmentResponse> | undefined;

export function parseEnvironmentCommand(value: unknown): EnvironmentCommand {
  return (commandSchema ??= schema<EnvironmentCommand>("EnvironmentCommand")).parse(value);
}

export function environmentFailure(error: unknown): EnvironmentReceipt {
  const message = (error instanceof Error ? error.message : String(error)).slice(0, 4096);
  if (error instanceof BrainError && ["ambiguous", "unknown"].includes(error.code)) return { type: "unknown", message };
  return { type: "failure", code: error instanceof BrainError ? error.code : "environment_error", message,
    retryable: error instanceof BrainError && error.retryable,
    ...(error instanceof BrainError && error.details !== undefined ? { details: error.details } : {}) };
}

/** Adapt provider operations to Brain's generated command and receipt contract. */
export function environmentProtocol<Context = void>(run: (operation: EnvironmentOperation, context: Context) => EnvironmentReceipt | Promise<EnvironmentReceipt>) {
  return async (raw: unknown, context: Context): Promise<EnvironmentResponse> => {
    const command = parseEnvironmentCommand(raw);
    let receipt;
    try { receipt = await run(command.operation, context); }
    catch (error) { receipt = environmentFailure(error); }
    return (responseSchema ??= schema<EnvironmentResponse>("EnvironmentResponse")).parse({
      contract: command.contract, sequence: command.operation.sequence, receipt,
    });
  };
}
