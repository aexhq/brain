import { readFile, writeFile } from "node:fs/promises";
import { join, basename } from "node:path";
import { z } from "zod";
import {
  createAgentSession, createExtensionRuntime, convertToLlm, ModelRuntime, SessionManager, SettingsManager,
  type AgentSession, type ResourceLoader, type ToolDefinition,
} from "@earendil-works/pi-coding-agent";
import type { AssistantMessage, Message as PiMessage, Model, UserMessage } from "@earendil-works/pi-ai";
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai/utils/event-stream";
import { getCurrentSystemPrompt, getCurrentTools } from "@earendil-works/pi-ai/utils/transcript";
import type { AgentHarness, HarnessContext } from "./harness.js";
import type { ContentBlock, Message, ModelResult, Usage } from "./generated/session.js";
import { activationText, catalogue, placedTools } from "./harness-bridge.js";

const optionsSchema = z.strictObject({
  contextWindow: z.number().int().positive().optional(),
  maxTokens: z.number().int().positive().optional(),
  compaction: z.strictObject({
    enabled: z.boolean().optional(), reserveTokens: z.number().int().positive().optional(),
    keepRecentTokens: z.number().int().positive().optional(),
  }).optional(),
});

type SavedMessage = PiMessage & { brainMessage?: Message; brainUsage?: Usage };

// Pi requires numeric counters for its own budgeting. Only Brain's original optional
// counters enter the journal; these estimates remain private to the upstream runtime.
function piUsage(usage: Usage, messages: unknown): AssistantMessage["usage"] {
  const input = usage.total_input_tokens ?? usage.input_tokens ?? Math.ceil(JSON.stringify(messages).length / 4);
  const output = usage.output_tokens ?? 0;
  return { input, output, cacheRead: 0, cacheWrite: 0, totalTokens: input + output,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } };
}

function toBrainMessages(messages: PiMessage[]): Message[] {
  return messages.filter(message => message.role !== "system").map(raw => {
    const message = raw as SavedMessage;
    if (message.brainMessage !== undefined) return message.brainMessage;
    if (message.role === "system") throw new Error("system message must be carried by the request prompt");
    const content = typeof message.content === "string" ? [{ type: "text" as const, text: message.content }] : message.content;
    if (message.role === "toolResult") {
      const saved = message.details as { brainResult?: ContentBlock } | undefined;
      return { role: "user", content: [saved?.brainResult ?? { type: "tool_result", tool_use_id: message.toolCallId,
        content: content.every(block => block.type === "text")
          ? content.map(block => block.type === "text" ? block.text : "").join("\n")
          : content, is_error: message.isError }] };
    }
    return { role: message.role === "assistant" ? "assistant" : "user", content: content.flatMap((block): ContentBlock[] => {
      if (block.type === "text") return [{ type: "text", text: block.text }];
      if (block.type === "toolCall") return [{ type: "tool_use", id: block.id, name: block.name, input: block.arguments }];
      if (block.type === "thinking") return block.thinking ? [{ type: "text", text: block.thinking }] : [];
      throw new Error("Pi produced unmediated media; use Brain media URLs");
    }) };
  });
}

function assistant(result: ModelResult, model: Model<any>): AssistantMessage & SavedMessage {
  return {
    role: "assistant", api: model.api, provider: model.provider, model: model.id, timestamp: Date.now(),
    stopReason: result.stop_reason === "tool_use" ? "toolUse" : result.stop_reason === "max_tokens" ? "length" : "stop",
    usage: piUsage(result.usage, result.message), brainMessage: result.message, brainUsage: result.usage,
    content: result.message.content.flatMap((block): AssistantMessage["content"] => {
      if (block.type === "text") return [{ type: "text", text: block.text }];
      if (block.type === "tool_use") return [{ type: "toolCall", id: block.id, name: block.name, arguments: block.input as Extract<AssistantMessage["content"][number], { type: "toolCall" }>["arguments"] }];
      if (block.type === "native") return [{ type: "thinking", thinking: "", thinkingSignature: JSON.stringify(block) }];
      throw new Error(`unsupported assistant content: ${block.type}`);
    }),
  };
}

function loader(system: string): ResourceLoader {
  return {
    getExtensions: () => ({ extensions: [], errors: [], runtime: createExtensionRuntime() }),
    getSkills: () => ({ skills: [], diagnostics: [] }), getPrompts: () => ({ prompts: [], diagnostics: [] }),
    getThemes: () => ({ themes: [], diagnostics: [] }), getAgentsFiles: () => ({ agentsFiles: [] }),
    getSystemPrompt: () => system, getSystemPromptSource: () => undefined,
    getAppendSystemPrompt: () => [], getAppendSystemPromptSources: () => [],
    extendResources() {}, async reload() {},
  };
}

/** The complete upstream Pi session SDK, with Brain as its provider and tool executor. */
export function piHarness(): AgentHarness {
  return {
    name: "pi", stateVersion: "1",
    async open({ stateDirectory, configuration }) {
      let session: AgentSession | undefined;
      let current: HarnessContext | undefined;
      let lastError: unknown;
      let newInput: Message | undefined;
      let unsubscribe: (() => void) | undefined;
      let initialCatalogue: string | undefined;
      async function start(ctx: HarnessContext) {
        const options = optionsSchema.parse({ ...(configuration as object ?? {}), ...(ctx.configuration as object ?? {}) });
        const contextWindow = options.contextWindow ?? ctx.modelInfo.context_window_tokens;
        const maxTokens = options.maxTokens ?? ctx.modelInfo.max_output_tokens;
        if (contextWindow === undefined || maxTokens === undefined) throw new Error("Pi requires model contextWindow and maxTokens; supply missing metadata in harness configuration");
        const runtime = await ModelRuntime.create({ authPath: join(stateDirectory, "auth.json"), modelsPath: null,
          allowModelNetwork: false, refreshOnCreate: false });
        runtime.registerProvider("brain", {
          api: "brain", baseUrl: "http://brain.invalid", apiKey: "brain-mediated",
          models: [{ id: "session", name: ctx.modelInfo.id, reasoning: ctx.modelInfo.reasoning ?? false,
            input: ["text"], cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, contextWindow, maxTokens }],
          streamSimple(model, context, streamOptions) {
            const stream = createAssistantMessageEventStream();
            void (async () => {
              try {
                if (current === undefined) throw new Error("model called outside an active Brain activation");
                const result = await current.model({
                  system: getCurrentSystemPrompt(context.messages), messages: toBrainMessages(context.messages),
                  tools: getCurrentTools(context.messages).map(tool => ({ name: tool.name, description: tool.description, input_schema: tool.parameters })),
                  ...(streamOptions?.maxTokens === undefined ? {} : { max_output_tokens: streamOptions.maxTokens }),
                });
                lastError = undefined;
                const message = assistant(result, model);
                stream.push({ type: "start", partial: { ...message, content: [] } });
                stream.push({ type: "done", reason: message.stopReason as "stop" | "length" | "toolUse", message });
                stream.end(message);
              } catch (error) {
                lastError = error;
                const message: AssistantMessage = { role: "assistant", api: model.api, provider: model.provider, model: model.id,
                  timestamp: Date.now(), content: [], usage: piUsage({}, []), stopReason: "error",
                  errorMessage: error instanceof Error ? error.message : String(error) };
                stream.push({ type: "error", reason: "error", error: message });
                stream.end(message);
              }
            })();
            return stream;
          },
        });
        let manager: SessionManager;
        const pointer = join(stateDirectory, "native-session.json");
        let file: string | undefined;
        try { file = JSON.parse(await readFile(pointer, "utf8")) as string; }
        catch (error) { if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error; }
        if (file !== undefined) {
          if (basename(file) !== file) throw new Error("invalid Pi native session pointer");
          const path = join(stateDirectory, file);
          await readFile(path); // SessionManager.open creates on missing files; resume must fail instead.
          manager = SessionManager.open(path, stateDirectory, stateDirectory);
        } else {
          manager = SessionManager.create(stateDirectory, stateDirectory);
          for (const message of ctx.transcript) {
            if (message.role === "assistant") manager.appendMessage(assistant({ message, usage: {}, stop_reason: "end_turn" }, runtime.getModel("brain", "session")!));
            else manager.appendMessage({ role: "user", content: message.content.filter(block => block.type === "text"),
              timestamp: Date.now(), brainMessage: message } as UserMessage & SavedMessage);
          }
          await writeFile(pointer, JSON.stringify(basename(manager.getSessionFile()!)));
        }
        const tools = placedTools(ctx.tools);
        ({ session } = await createAgentSession({
          cwd: stateDirectory, agentDir: stateDirectory, modelRuntime: runtime, model: runtime.getModel("brain", "session"),
          resourceLoader: loader(ctx.system), thinkingLevel: "off", sessionManager: manager,
          settingsManager: SettingsManager.inMemory({ retry: { enabled: false }, compaction: options.compaction }),
          tools: tools.map(tool => tool.name),
          customTools: tools.map(tool => ({ name: tool.name, label: tool.name, description: tool.description,
            parameters: tool.input_schema as ToolDefinition["parameters"],
            async execute(id, input) {
              if (current === undefined) throw new Error("tool called outside an active Brain activation");
              const result = await tool.execute(current, id, input);
              if (result.is_error) throw new Error(typeof result.content === "string" ? result.content : JSON.stringify(result.content));
              return { content: [{ type: "text", text: typeof result.content === "string" ? result.content : JSON.stringify(result.content) }], details: { brainResult: result } };
            },
          })),
        }));
        const actual = session.getActiveToolNames().sort();
        initialCatalogue = catalogue(ctx.tools);
        if (JSON.stringify(actual) !== JSON.stringify(tools.map(tool => tool.name).sort())) throw new Error("Pi exposed an unexpected tool");
        // Preserve Brain's URL media and native continuation objects in upstream native
        // storage without fetching media or turning projection writes into execution state.
        unsubscribe = session.agent.subscribe(event => {
          if (event.type === "message_start" && event.message.role === "user" && newInput !== undefined) {
            (event.message as UserMessage & SavedMessage).brainMessage = newInput;
            newInput = undefined;
          }
        });
      }
      return {
        async run(ctx) {
          current = ctx;
          lastError = undefined;
          const abort = () => session?.agent.abort();
          ctx.signal.addEventListener("abort", abort, { once: true });
          try {
            if (session === undefined) await start(ctx);
            ctx.signal.throwIfAborted();
            if (catalogue(ctx.tools) !== initialCatalogue) throw new Error("Pi requires fixed Tool placements for the native session");
            const text = await activationText(ctx);
            if (text === undefined) return;
            newInput = { role: "user", content: [{ type: "text", text }, ...(ctx.input?.media ?? [])] };
            await session!.prompt(text, { expandPromptTemplates: false });
            if (lastError !== undefined) throw lastError;
            ctx.signal.throwIfAborted();
            const transcript = toBrainMessages(convertToLlm(session!.messages));
            await ctx.setTranscript(transcript);
            const last = transcript.findLast(message => message.role === "assistant");
            const message = last?.content.filter(block => block.type === "text").map(block => block.text).join("") ?? "";
            if (message) await ctx.emit("output_emitted", { type: "assistant_message", message });
          } finally { ctx.signal.removeEventListener("abort", abort); current = undefined; newInput = undefined; }
        },
        cancel() { session?.agent.abort(); },
        close() { unsubscribe?.(); session?.dispose(); },
      };
    },
  };
}
