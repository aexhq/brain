import * as host from "brain:agentloop/host@0.2.0";
import { createAgentloopContext } from "./agentloop-context.js";
import type { AgentloopHandler } from "./agentloop-context.js";
import type { RuntimeEnvelope } from "./generated/session.js";

export type { AgentloopContext, AgentloopHandler } from "./agentloop-context.js";

interface ComponentInput {
  inputJson: string;
  transcriptJson: string;
  kvJson: string;
  eventsJson: string;
  configurationJson: string;
  toolsJson: string;
  system: string;
  runtime: { logicalTimeMs: bigint; deterministicSeed: Uint8Array };
}

function json(value: unknown): string {
  const encoded = JSON.stringify(value);
  if (encoded === undefined) throw new TypeError("Brain service input must be a JSON value");
  return encoded;
}

async function request(method: string, input: unknown): Promise<unknown> {
  try {
    switch (method) {
      case "model": return JSON.parse(host.model(json(input)));
      case "dispatch": return JSON.parse(host.dispatch(json(input)));
      case "events": return JSON.parse(host.events(BigInt(input as number)));
      case "set_transcript": return Number(host.setTranscript(json(input)));
      case "kv_read": {
        const value = host.kvRead(input as string);
        return value === undefined ? {} : { value: JSON.parse(value) };
      }
      case "kv_put": {
        const { key, value } = input as { key: string; value: unknown };
        return Number(host.kvPut(key, json(value)));
      }
      case "kv_delete": return Number(host.kvDelete(input as string));
      case "emit": {
        const { event_type, data } = input as { event_type: string; data: unknown };
        return Number(host.emit(event_type, json(data)));
      }
      case "environments": return JSON.parse(host.environments(json(input)));
      case "telemetry": return host.telemetry(json(input));
      default: throw new TypeError(`Unknown Agentloop service: ${method}`);
    }
  } catch (error) {
    const payload = (error as { payload?: { code: string; message: string; retryable: boolean } })?.payload;
    if (payload) throw Object.assign(new Error(payload.message), payload);
    throw error;
  }
}

/** Exports a JavaScript handler through Brain's Component contract. */
export function defineAgentloop(handler: AgentloopHandler) {
  return async function turn(input: ComponentInput): Promise<{ resultJson: string | undefined }> {
    try {
      const result = await handler(createAgentloopContext({
        input: JSON.parse(input.inputJson) ?? undefined,
        transcript: JSON.parse(input.transcriptJson),
        kv: JSON.parse(input.kvJson),
        events: JSON.parse(input.eventsJson),
        configuration: JSON.parse(input.configurationJson),
        tools: JSON.parse(input.toolsJson),
        system: input.system,
        runtime: {
          logical_time_ms: Number(input.runtime.logicalTimeMs),
          deterministic_seed: Array.from(input.runtime.deterministicSeed) as RuntimeEnvelope["deterministic_seed"],
        },
      }, request));
      return { resultJson: result === undefined ? undefined : json(result) };
    } catch (error) {
      const source = error as { code?: string; message?: string; retryable?: boolean };
      const failure = new Error(source?.message ?? String(error));
      throw Object.assign(failure, { payload: {
        code: source?.code ?? "agentloop_failed",
        message: failure.message,
        retryable: source?.retryable ?? false,
      } });
    }
  };
}
