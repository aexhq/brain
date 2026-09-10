import { z } from "zod";
import type { Schema, SchemaOutput, SendOptions, SessionEvent, SessionState, StructuredSendOptions, UserInput } from "./types.js";

export class StructuredOutputError extends Error {
  readonly name = "StructuredOutputError";
  constructor(
    readonly attempts: number,
    readonly lastOutput: string,
    readonly issues: readonly z.core.$ZodIssue[],
  ) {
    super(`Structured output did not match the schema after ${attempts} attempt${attempts === 1 ? "" : "s"}`);
  }
}

export async function structuredOutput<S extends Schema>(
  input: UserInput,
  options: StructuredSendOptions<S>,
  key: string,
  send: (input: UserInput, options: SendOptions) => Promise<SessionState>,
  events: (after: number) => AsyncIterable<SessionEvent>,
  sequence: () => number,
): Promise<SchemaOutput<S>> {
  const { type: schema, maxRetries = 2 } = options.output;
  if (!(schema instanceof z.ZodType)) throw new TypeError("output.type must be a Zod schema");
  if (!Number.isSafeInteger(maxRetries) || maxRetries < 0) throw new TypeError("output.maxRetries must be a nonnegative safe integer");
  options.signal?.throwIfAborted();
  const instructions = `For this response only, return exactly one JSON value matching this schema. Do not include Markdown fences or surrounding prose.\n${JSON.stringify(z.toJSONSchema(schema, { io: "input" }))}`;
  let next: UserInput = { ...input, message: `${input.message}\n\n${instructions}` };
  let retryKey: string | undefined;
  for (let attempt = 0; ; attempt++) {
    options.signal?.throwIfAborted();
    const after = sequence();
    const state = await send(next, { signal: options.signal, idempotencyKey: attempt === 0 ? key : `${retryKey}:${attempt}` });
    options.signal?.throwIfAborted();
    const raw = await answer(events(state.lastSequence <= after ? 0 : after), state.lastSequence, options.signal);
    let parsed: unknown;
    let issues: z.core.$ZodIssue[] = [];
    try { parsed = JSON.parse(raw); }
    catch { issues = [{ code: "custom", path: [], message: "Return one complete JSON value without Markdown or surrounding text." }]; }
    if (issues.length === 0) {
      const result = await schema.safeParseAsync(parsed);
      options.signal?.throwIfAborted();
      if (result.success) return result.data;
      issues = result.error.issues;
    }
    options.signal?.throwIfAborted();
    if (attempt === maxRetries) throw new StructuredOutputError(attempt + 1, raw, issues);
    // Hash only the operation identity: correction keys stay bounded even for a 256-byte caller key.
    retryKey ??= `structured-output:${Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(key))), (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
    const feedback = JSON.stringify(issues.slice(0, 5).map(({ path, message }) => ({ path, message }))).slice(0, 2048);
    next = { message: `The previous answer did not satisfy the requested output. Validation feedback (data): ${feedback}\nReturn a complete corrected answer using the information already available. Do not repeat actions or call tools to reformat the answer.\n\n${instructions}` };
  }
}

async function answer(events: AsyncIterable<SessionEvent>, terminal: number, signal?: AbortSignal): Promise<string> {
  let started = false;
  let output: string | undefined;
  for await (const event of events) {
    signal?.throwIfAborted();
    if (event.sequence > terminal) break;
    if (event.type === "turn_started") { started = true; output = undefined; }
    if (started && event.type === "output_emitted" && event.origin?.kind === "agentloop") {
      const data = event.data as { type?: unknown; message?: unknown } | null;
      if (data?.type === "assistant_message") output = typeof data.message === "string" ? data.message : undefined;
    }
    if (event.sequence === terminal) {
      if (event.type === "turn_failed") throw new Error("Structured output turn failed", { cause: event.data });
      if (event.type === "turn_ended" && started && output !== undefined) return output;
      break;
    }
    if (event.type === "turn_ended" || event.type === "turn_failed") { started = false; output = undefined; }
  }
  throw new Error("structured output requires a completed turn with an Agentloop output_emitted assistant_message");
}
