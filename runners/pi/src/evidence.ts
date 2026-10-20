import { randomBytes } from "node:crypto";
import { createAssistantMessageEventStream, type AssistantMessage, type AssistantMessageEventStream, type ModelsSimpleStreamOptions } from "@earendil-works/pi-ai";
import type { AgentSession } from "@earendil-works/pi-coding-agent";
import type { ModelRequest, ModelUsage } from "./generated.js";

const id = () => randomBytes(16).toString("hex");
const unknown = (): ModelUsage => ({ availability: "unknown", semantics: "snapshot", input_cache: "unknown", output_reasoning: "unknown" });
const object = (value: unknown): Record<string, unknown> => value && typeof value === "object" ? value as Record<string, unknown> : {};
const field = (value: unknown, name: string) => object(value)[name];

class RequestEvidence {
  readonly logical: ModelRequest;
  attempt: ModelRequest | undefined;
  attempts = 0;
  seen = new Set<string>();
  constructor(purpose: string, private observe: (request: ModelRequest) => Promise<void>) {
    this.logical = { request_id: id(), boundary: "request", purpose, state: "started", harness: "", provider: "", model: "", attempts: "unobserved", usage: unknown() };
  }
  capture(data: unknown) {
    const usage = object(field(data, "usage") ?? field(field(data, "response"), "usage") ?? field(field(data, "message"), "usage") ?? field(data, "usageMetadata"));
    const note = (key: string, ...values: unknown[]) => {
      for (const value of values) if (value !== undefined && value !== null) {
        if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw new Error("invalid provider usage");
        this.seen.add(key);
      }
    };
    note("input", usage.prompt_tokens, usage.input_tokens, usage.promptTokenCount);
    note("output", usage.completion_tokens, usage.output_tokens, usage.candidatesTokenCount);
    note("read", field(usage.prompt_tokens_details, "cached_tokens"), field(usage.input_tokens_details, "cached_tokens"), usage.cache_read_input_tokens, usage.cachedContentTokenCount, usage.prompt_cache_hit_tokens, usage.cached_tokens);
    note("write", field(usage.prompt_tokens_details, "cache_write_tokens"), usage.cache_creation_input_tokens);
    note("reasoning", field(usage.completion_tokens_details, "reasoning_tokens"), field(usage.output_tokens_details, "reasoning_tokens"), usage.thoughtsTokenCount);
  }
  async fetch(input: Parameters<typeof fetch>[0], init?: Parameters<typeof fetch>[1]): Promise<Response> {
    if (this.attempt) { this.attempt.state = "unknown"; await this.observe(this.attempt); }
    this.seen.clear(); this.attempts++;
    this.logical.attempts = "observed";
    const attempt = { ...this.logical, attempt_id: id(), usage: unknown() };
    this.attempt = attempt;
    await this.observe(attempt);
    try {
      const response = await fetch(input, init);
      attempt.provider_request_id = response.headers.get("x-request-id") ?? response.headers.get("request-id") ?? undefined;
      if (!response.ok) { attempt.state = "failed"; await this.observe(attempt); this.attempt = undefined; }
      return response;
    } catch (error) {
      attempt.state = init?.signal?.aborted ? "cancelled" : "unknown";
      await this.observe(attempt); this.attempt = undefined;
      throw error;
    }
  }
  async finish(message?: AssistantMessage, api?: string) {
    const state = message?.stopReason === "aborted" ? "cancelled" : !message || message.stopReason === "error" ? "failed" : "completed";
    const usage = unknown();
    if (message && this.seen.size) {
      usage.availability = state === "completed" && this.seen.has("input") && this.seen.has("output") ? "reported" : "partial";
      if (this.seen.has("input")) usage.input_tokens = message.usage.input;
      if (this.seen.has("output")) usage.output_tokens = message.usage.output;
      if (this.seen.has("read")) usage.cache_read_tokens = message.usage.cacheRead;
      if (this.seen.has("write")) usage.cache_write_tokens = message.usage.cacheWrite;
      if (this.seen.has("reasoning") && message.usage.reasoning !== undefined) usage.reasoning_tokens = message.usage.reasoning;
      usage.input_cache = api === "anthropic-messages" || this.seen.has("read") || this.seen.has("write") ? "excluded" : api?.startsWith("openai-") ? "included" : "unknown";
      usage.output_reasoning = "included"; // Pi 1.2.0's Usage.output includes reasoning.
    }
    if (this.attempt) { this.attempt.state = state; this.attempt.usage = usage; await this.observe(this.attempt); this.attempt = undefined; }
    this.logical.state = state;
    // The final message describes the final attempt, not the cost of failed retries.
    this.logical.usage = this.attempts <= 1 ? usage : unknown();
    await this.observe(this.logical);
  }
}

// Use the same pinned stream seam for generations, native retries and compaction.
// HTTP fetch injection observes SDK attempts; other transports retain an explicit gap.
export function observeRequests(session: AgentSession, observe: (request: ModelRequest) => Promise<void>) {
  const stream = session.agent.streamFunction;
  session.agent.streamFunction = async (model, context, options) => {
    const evidence = new RequestEvidence(session.isCompacting ? "compaction" : "primary", observe);
    await observe(evidence.logical);
    const requestOptions: ModelsSimpleStreamOptions = { ...options, transport: "sse",
      fetch: (input, init) => evidence.fetch(input, init),
      onProviderStreamEvent: async (data, selected) => { evidence.capture(data); await options?.onProviderStreamEvent?.(data, selected); },
    };
    let source: AssistantMessageEventStream;
    try { source = await stream(model, context, requestOptions); }
    catch (error) { await evidence.finish(undefined, model.api); throw error; }
    const output = createAssistantMessageEventStream();
    void (async () => {
      try {
        for await (const event of source) {
          if (event.type === "done" || event.type === "error") await evidence.finish(event.type === "done" ? event.message : event.error, model.api);
          output.push(event);
        }
      } catch {
        // Do not expose provider bodies or credentials through an observation error.
        output.push({ type: "error", reason: "error", error: { role: "assistant", content: [], api: model.api, provider: model.provider, model: model.id,
          usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } }, stopReason: "error", errorMessage: "model observation failed", timestamp: Date.now() } });
      }
    })();
    return output;
  };
}
