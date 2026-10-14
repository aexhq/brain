import { z } from "zod";
import { environmentDefinitions } from "./generated/environment-schema.js";
import type { ActivationTool, ContentBlock, Media, ToolResult, ToolReturn } from "./generated/session.js";
import type { HarnessContext } from "./harness.js";

export type ResultBlock = Extract<ContentBlock, { type: "tool_result" }>;
const mediaSchema = z.fromJSONSchema({ $defs: environmentDefinitions, $ref: "#/$defs/Media" });
const observedKey = "brain.harness.observed";
const observations = new WeakMap<HarnessContext, { seen: Promise<Set<number>>; pending: Promise<unknown> }>();
function observed(context: HarnessContext) {
  let state = observations.get(context);
  if (state === undefined) {
    state = { seen: context.kv.get<number[]>(observedKey).then(value => new Set(value ?? [])), pending: Promise.resolve() };
    observations.set(context, state);
  }
  return state;
}
async function consume(context: HarnessContext, sequences: number[]) {
  const state = observed(context);
  state.pending = state.pending.then(async () => {
    const seen = await state.seen;
    sequences.forEach(sequence => seen.add(sequence));
    await context.kv.set(observedKey, [...seen]);
  });
  await state.pending;
}

export function catalogue(tools: ActivationTool[]): string {
  return JSON.stringify(tools.map(tool => ({ name: tool.name, environments: tool.environments })));
}

/** Preserve all returned observations, including execution that continues in the background. */
export function toolResult(returned: ToolReturn): ResultBlock {
  const results: { content: unknown; is_error: boolean }[] = [];
  const media: Media[] = [];
  let terminal: Record<string, unknown> | undefined;
  for (const event of returned.events) {
    const data = event.data as { result: ToolResult; outcome: Record<string, unknown> };
    if (event.event_type === "tool_result_emitted") {
      const output = data.result.output as { type?: string; version?: number; content?: unknown; media?: unknown[] } | null;
      if (!data.result.is_error && output?.type === "aex_tool_output" && output.version === 1) {
        if (!Array.isArray(output.media)) throw new Error("aex_tool_output requires media array");
        media.push(...output.media.map(value => mediaSchema.parse(value) as Media));
        results.push({ content: data.result.content ?? output.content, is_error: false });
      } else results.push({ content: data.result.content ?? data.result.output, is_error: data.result.is_error });
    }
    if (event.event_type === "tool_call_ended") terminal = data.outcome;
  }
  if (terminal !== undefined && terminal.status !== "ok") {
    results.push({ content: terminal.status === "error" ? terminal.error : terminal, is_error: true });
  }
  return {
    type: "tool_result", tool_use_id: returned.call_id,
    content: returned.finished && results.length === 1 ? results[0]!.content
      : { status: returned.finished ? "finished" : "running", results },
    is_error: results.some(result => result.is_error),
    ...(media.length ? { media } : {}),
  };
}

/** One visible tool per admitted placement; no upstream built-in tool is exposed. */
export function placedTools(tools: ActivationTool[]) {
  const names = new Set<string>();
  return tools.flatMap(tool => tool.environments.map((environment, index) => {
    const name = tool.environments.length === 1 ? tool.name : `${tool.name.slice(0, 110)}__env${index + 1}`;
    if (names.has(name)) throw new Error(`harness tool name collision: ${name}`);
    names.add(name);
    return {
      name, description: `${tool.description}${tool.environments.length === 1 ? "" : ` (Environment: ${environment})`}`,
      input_schema: tool.input_schema,
      async execute(context: HarnessContext, callId: string, input: unknown) {
        const available = context.tools.find(current => current.name === tool.name && current.environments.includes(environment));
        if (available === undefined) throw new Error(`Tool ${tool.name} is no longer available in ${environment}`);
        const reference = available.environment_refs?.find(ref => ref.name === environment);
        const returned = await context.callTool({ call_id: callId, name: tool.name, environment, input,
          ...(reference === undefined ? {} : { environment_sequence: reference.sequence }) });
        await consume(context, returned.events.map(event => event.sequence));
        return toolResult(returned);
      },
    };
  }));
}

export async function activationText(context: HarnessContext): Promise<string | undefined> {
  const seen = await observed(context).seen;
  const observations = context.events.filter(event => !seen.has(event.sequence) && [
    "tool_result_emitted", "tool_call_ended", "environment_observation", "environment_closed",
    "environment_unreachable", "turn_failed",
  ].includes(event.event_type));
  if (context.events.some(event => seen.has(event.sequence))) {
    context.events.forEach(event => seen.delete(event.sequence));
    await context.kv.set(observedKey, [...seen]);
  }
  const text = observations.length === 0 ? "" : `Runtime observations (data):\n${JSON.stringify(observations)}\n\n`;
  return context.input === undefined && !text ? undefined : `${text}${context.input?.message ?? "Process the runtime observations."}`;
}
