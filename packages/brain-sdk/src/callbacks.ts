import { z } from "zod";
import { BrainError } from "./errors.js";
import type { EnvironmentEvent, EnvironmentOperation, EnvironmentReceipt, ExecutionCallback } from "./generated/session.js";

export interface CallbackOptions {
  fetch?: typeof globalThis.fetch;
  signal?: AbortSignal;
}

const sequence = z.number().int().positive().safe();

async function post(callback: ExecutionCallback, input: unknown, options: CallbackOptions): Promise<unknown> {
  let response: Response;
  try {
    response = await (options.fetch ?? globalThis.fetch)(callback.url, {
      method: "POST", redirect: "error",
      headers: { authorization: `Bearer ${callback.token}`, "content-type": "application/json" },
      body: JSON.stringify(input), ...(options.signal === undefined ? {} : { signal: options.signal }),
    });
  } catch {
    throw new BrainError(0, "ambiguous", "Callback outcome is unknown: no response was received", false);
  }
  let body;
  try { body = await response.json(); }
  catch { throw new BrainError(response.status, "ambiguous", "Callback outcome is unknown: invalid acknowledgement", false); }
  if (!response.ok) throw new BrainError(response.status, body?.code ?? "http_error", body?.message ?? response.statusText, body?.retryable ?? false, body?.details);
  return body;
}

/** Call only a service granted to this invocation. No callback request is retried. */
export function callExecution(callback: ExecutionCallback | undefined, method: string, input: unknown, options: CallbackOptions = {}): Promise<unknown> {
  options.signal?.throwIfAborted();
  if (!callback?.methods.includes(method)) throw new BrainError(403, "not_granted", `Execution service ${method} is not granted`, false);
  return post(callback, { method, input }, options);
}

/** A binding reporter has its own lifetime and cannot call execution services. */
export async function reportEnvironment(callback: ExecutionCallback | undefined, event: EnvironmentEvent, options: CallbackOptions = {}): Promise<number> {
  options.signal?.throwIfAborted();
  const method = event.type === "event" ? "emit" : "result";
  if (!callback?.methods.includes(method)) throw new BrainError(403, "not_granted", `Environment reporter ${method} is not granted`, false);
  const result = await post(callback, event, options);
  return z.object({ sequence }).parse(result).sequence;
}

/** Complete a synchronous Tool result; loss of its commit acknowledgement is uncertain. */
export async function finishExecution(operation: EnvironmentOperation, receipt: EnvironmentReceipt, options: CallbackOptions = {}): Promise<EnvironmentReceipt> {
  if (receipt.type !== "result") return receipt;
  if (operation.request.type !== "execute") throw new TypeError("Tool completion requires an execute operation");
  try {
    sequence.parse(await callExecution(operation.request.callback, "finish", { status: "ok", value: receipt.output }, options));
    return receipt;
  } catch (error) {
    return { type: "unknown", message: `Tool completion acknowledgement was lost: ${error instanceof Error ? error.message : String(error)}`.slice(0, 4096) };
  }
}
