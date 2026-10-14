import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { OpenCode, Tool } from "@opencode/sdk";
import type { LanguageModelV3, LanguageModelV3CallOptions, LanguageModelV3Content, LanguageModelV3StreamPart } from "@ai-sdk/provider";
import { z } from "zod";
import { HarnessServiceError, type AgentHarness, type HarnessContext } from "./harness.js";
import { activationText, catalogue, placedTools } from "./harness-bridge.js";
import type { ContentBlock, Message, ModelResult } from "./generated/session.js";

const optionsSchema = z.strictObject({
  contextWindow: z.number().int().positive().optional(), maxTokens: z.number().int().positive().optional(),
  compaction: z.strictObject({ auto: z.boolean().optional(), buffer: z.number().int().nonnegative().optional(),
    keep: z.strictObject({ tokens: z.number().int().nonnegative().optional() }).optional() }).optional(),
});

function promptMessages(prompt: LanguageModelV3CallOptions["prompt"]): Message[] {
  return prompt.filter(message => message.role !== "system").map(message => ({
    role: message.role === "assistant" ? "assistant" : "user",
    content: message.content.map((part): ContentBlock => {
      const native = "providerOptions" in part ? part.providerOptions?.brain?.block : undefined;
      if (typeof native === "string") return JSON.parse(native) as ContentBlock;
      switch (part.type) {
        case "text": case "reasoning": return { type: "text", text: part.text };
        case "tool-call": return { type: "tool_use", id: part.toolCallId, name: part.toolName, input: part.input };
        case "tool-result": return { type: "tool_result", tool_use_id: part.toolCallId,
          content: part.output, is_error: part.output.type.startsWith("error") };
        default: throw new Error("OpenCode harness currently supports text inputs; use Pi for URL media");
      }
    }),
  }));
}

function resultContent(result: ModelResult): LanguageModelV3Content[] {
  return result.message.content.map(block => {
    switch (block.type) {
      case "text": return { type: "text", text: block.text };
      case "native": return { type: "reasoning", text: "", providerMetadata: { brain: { block: JSON.stringify(block) } } };
      case "tool_use": return { type: "tool-call", toolCallId: block.id, toolName: block.name, input: JSON.stringify(block.input) };
      default: throw new Error(`unsupported OpenCode assistant content: ${block.type}`);
    }
  });
}

/** Upstream OpenCode owns scheduling, compaction, and its SQLite session database. */
export function openCodeHarness(): AgentHarness {
  return {
    name: "opencode", stateVersion: "2",
    async open({ stateDirectory, configuration }) {
      let client: Awaited<ReturnType<typeof OpenCode.create>> | undefined;
      let sessionID: string | undefined;
      let current: HarnessContext | undefined;
      let failure: unknown;
      let projection: Message[] = [];
      let primary = true;
      let initialCatalogue: string | undefined;
      const active = () => {
        if (failure !== undefined) throw failure;
        if (current === undefined) throw new Error("OpenCode attempted execution outside an active Brain activation");
        current.signal.throwIfAborted();
        return current;
      };
      async function generate(options: LanguageModelV3CallOptions) {
        const ctx = active();
        const messages = promptMessages(options.prompt);
        const tools = options.tools ?? [];
        const admitted = new Set(placedTools(ctx.tools).map(tool => `brain_${tool.name}`));
        if (tools.some(tool => tool.type !== "function" || !admitted.has(tool.name))) throw new Error("OpenCode offered a tool outside Brain's catalogue");
        let result: ModelResult;
        try {
          result = await ctx.model({
            messages, system: options.prompt.filter(message => message.role === "system").map(message => message.content).join("\n\n"),
            tools: tools.map(tool => {
              if (tool.type !== "function") throw new Error("provider-executed tools are not supported");
              return { name: tool.name, description: tool.description ?? "", input_schema: tool.inputSchema };
            }),
            ...(options.maxOutputTokens === undefined ? {} : { max_output_tokens: options.maxOutputTokens }),
          });
        } catch (error) { failure = error; throw error; }
        for (const block of result.message.content) {
          if (block.type === "tool_use" && !tools.some(tool => tool.name === block.name)) throw new Error(`model requested an unoffered tool: ${block.name}`);
        }
        if (primary) projection = [...messages, result.message];
        const usage = result.usage;
        return {
          content: resultContent(result), warnings: [],
          finishReason: { unified: result.stop_reason === "tool_use" ? "tool-calls" as const : "stop" as const, raw: result.stop_reason },
          usage: { inputTokens: { total: usage.total_input_tokens ?? usage.input_tokens, noCache: usage.input_tokens,
            cacheRead: usage.cache_read_input_tokens, cacheWrite: usage.cache_creation_input_tokens },
          outputTokens: { total: usage.output_tokens, text: undefined, reasoning: usage.reasoning_tokens } },
        };
      }
      const model: LanguageModelV3 = {
        specificationVersion: "v3", provider: "brain", modelId: "session", supportedUrls: {},
        doGenerate: generate,
        async doStream(options) {
          const result = await generate(options);
          return { stream: new ReadableStream<LanguageModelV3StreamPart>({ start(controller) {
            controller.enqueue({ type: "stream-start", warnings: [] });
            result.content.forEach((content, index) => {
              const id = String(index);
              if (content.type === "tool-call") controller.enqueue(content);
              else if (content.type === "text" || content.type === "reasoning") {
                const type = content.type;
                controller.enqueue({ type: `${type}-start`, id, providerMetadata: content.providerMetadata });
                controller.enqueue({ type: `${type}-delta`, id, delta: content.text });
                controller.enqueue({ type: `${type}-end`, id, providerMetadata: content.providerMetadata });
              }
            });
            controller.enqueue({ type: "finish", finishReason: result.finishReason, usage: result.usage });
            controller.close();
          } }) };
        },
      };
      async function start(ctx: HarnessContext) {
        const options = optionsSchema.parse({ ...(configuration as object ?? {}), ...(ctx.configuration as object ?? {}) });
        const contextWindow = options.contextWindow ?? ctx.modelInfo.context_window_tokens;
        const maxTokens = options.maxTokens ?? ctx.modelInfo.max_output_tokens;
        if (contextWindow === undefined || maxTokens === undefined) throw new Error("OpenCode requires contextWindow and maxTokens model metadata");
        const tools = placedTools(ctx.tools).map(tool => ({ ...tool, name: `brain_${tool.name}` }));
        if (tools.some(tool => !/^[A-Za-z0-9_-]+$/u.test(tool.name))) throw new Error("OpenCode tool names must contain only letters, digits, underscores, and hyphens");
        const allowed = new Set(tools.map(tool => tool.name));
        initialCatalogue = catalogue(ctx.tools);
        client = await OpenCode.create({
          database: { path: join(stateDirectory, "opencode.db") },
          config: { project: false, directory: stateDirectory, content: JSON.stringify({
            model: { providerID: "brain", id: "session" }, update: "disable", share: "disabled", snapshots: false, warming: false,
            compaction: options.compaction, permissions: [{ action: "*", resource: "*", effect: "deny" },
              ...tools.map(tool => ({ action: tool.name, resource: "*", effect: "allow" }))],
            providers: { brain: { name: "Brain", package: `aisdk:${new URL("./harness-opencode-provider.js", import.meta.url).href}`,
              models: { session: { name: ctx.modelInfo.id, limit: { context: contextWindow, output: maxTokens } } } } },
          }) }, models: { fetch: false, snapshot: false }, fs: { filewatcher: false, fff: false },
          plugins: [{ id: "brain.harness", async setup(plugin) {
            await plugin.aisdk.hook("sdk", event => { event.sdk = { languageModel: () => model }; });
            await plugin.aisdk.hook("language", event => { event.language = model; });
            await plugin.session.hook("retry", event => { event.decision = { retry: false }; });
            await plugin.session.hook("title", event => { event.result = "Brain session"; });
            await plugin.session.hook("http.request", () => { throw new Error("direct OpenCode model HTTP is forbidden"); });
            await plugin.session.hook("experimental.ws.handshake", () => { throw new Error("direct OpenCode model WebSocket is forbidden"); });
            await plugin.session.hook("context", event => {
              primary = true;
              for (const name of Object.keys(event.tools)) if (!allowed.has(name)) delete event.tools[name];
              event.system = [{ type: "text", text: active().system || "Use the provided tools to help the user." }];
            });
            await plugin.session.hook("compaction", event => { primary = false; event.tools = {}; });
            await plugin.tool.hook("execute.before", event => {
              active();
              if (!allowed.has(event.tool)) throw new Tool.Error({ message: "Tool is not admitted by Brain" });
            });
            await plugin.tool.transform(editor => {
              for (const tool of editor.list()) editor.remove(tool.id);
              for (const tool of tools) editor.add({ name: tool.name, description: tool.description, input: tool.input_schema,
                options: { codemode: false }, async execute(input, call) {
                  try {
                    const result = await tool.execute(active(), call.id, input);
                    if (result.media?.length) throw new Error("OpenCode harness does not support Tool media; use Pi");
                    if (result.is_error) throw new Tool.Error({ message: JSON.stringify(result.content) });
                    return { content: typeof result.content === "string" ? result.content : JSON.stringify(result.content) };
                  } catch (error) {
                    if (error instanceof HarnessServiceError && error.fatal) failure = error;
                    throw error;
                  }
                } });
            });
          } }],
        });
        const pointer = join(stateDirectory, "native-session.json");
        try { sessionID = JSON.parse(await readFile(pointer, "utf8")) as string; }
        catch (error) { if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error; }
        if (sessionID === undefined) {
          if (ctx.transcript.length) throw new Error("OpenCode starts from native state; importing a Brain transcript is unsupported");
          sessionID = (await client.sessions.create({ location: { directory: stateDirectory }, model: { providerID: "brain", id: "session" } })).id;
          await writeFile(pointer, JSON.stringify(sessionID));
        } else await client.sessions.get({ sessionID });
      }
      return {
        async run(ctx) {
          current = ctx; failure = undefined; projection = [];
          try {
            if (ctx.input?.media?.length) throw new Error("OpenCode harness currently supports text inputs; use Pi for URL media");
            if (client === undefined) await start(ctx);
            if (catalogue(ctx.tools) !== initialCatalogue) throw new Error("OpenCode requires fixed Tool placements for the native session");
            const text = await activationText(ctx);
            if (text === undefined) return;
            await client!.sessions.prompt({ sessionID: sessionID!, text });
            await client!.sessions.wait({ sessionID: sessionID! });
            if (failure !== undefined) throw failure;
            ctx.signal.throwIfAborted();
            const messages = await client!.sessions.context({ sessionID: sessionID! });
            const last = messages.at(-1);
            if (last?.type === "idle" && last.outcome !== "succeeded") throw new Error(`OpenCode activation ${last.outcome}: ${JSON.stringify(last)}`);
            if (projection.length) await ctx.setTranscript(projection);
            const message = projection.findLast(message => message.role === "assistant")?.content
              .filter(block => block.type === "text").map(block => block.text).join("");
            if (message) await ctx.emit("output_emitted", { type: "assistant_message", message });
          } finally { current = undefined; }
        },
        async cancel() { if (client && sessionID) await client.sessions.interrupt({ sessionID }); },
        async close() {
          await client?.close(); client = undefined;
          // Bun finalizes SQLite statements during GC; Windows cannot unlink the
          // native database until those handles close.
          if (process.platform === "win32") {
            const { Bun } = globalThis as typeof globalThis & { Bun: { gc(force: boolean): void } };
            Bun.gc(true);
          }
        },
      };
    },
  };
}
