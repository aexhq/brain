// The official pi loop: real pinned @earendil-works/pi-agent-core driving the session's
// turns, adapted onto contracts/agentloop/v1 (design HX10: ctx is the adapter target). The
// kernel owns provider execution, custody and journaling; pi owns the policy. The host
// returns FOLDED model rounds, and this adapter synthesizes pi's event protocol from them.
import { runAgentLoop } from "@earendil-works/pi-agent-core";
import { AssistantMessageEventStream } from "@earendil-works/pi-ai";
import { defineAgentloop } from "@aexhq/agentloop";

const piModel = (model) => ({
  id: model,
  name: model,
  provider: "aex-sealed",
  api: "aex-loophost",
  baseUrl: "sealed",
  contextWindow: 128000,
  maxTokens: 8192,
  reasoning: false,
  input: ["text"],
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
});

const usageFromView = (usage) => ({
  input: usage?.input_tokens ?? 0,
  output: usage?.output_tokens ?? 0,
  cacheRead: usage?.cache_read_tokens ?? 0,
  cacheWrite: usage?.cache_write_tokens ?? 0,
  totalTokens:
    usage?.total_tokens ?? (usage?.input_tokens ?? 0) + (usage?.output_tokens ?? 0),
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
});

const textOf = (content) =>
  content
    .filter((block) => block.type === "text")
    .map((block) => block.text)
    .join("");

/** Rebuild one pi conversation message from a hydration journal view. */
function piFromView(view) {
  switch (view.type) {
    case "user_message":
      return { role: "user", content: textOf(view.content), timestamp: 0 };
    case "assistant_message": {
      const message = view.message;
      return {
        role: "assistant",
        content: message.content.map((block) =>
          block.type === "text"
            ? { type: "text", text: block.text }
            : {
                type: "toolCall",
                id: block.tool_call_id,
                name: block.name,
                arguments: block.input,
              },
        ),
        api: "aex-loophost",
        provider: "aex-sealed",
        model: message.model,
        usage: usageFromView(message.usage),
        stopReason: message.stop_reason === "tool_use" ? "toolUse" : "stop",
        timestamp: 0,
      };
    }
    case "tool_result":
      return {
        role: "toolResult",
        toolCallId: view.result.tool_call_id,
        toolName: view.result.name,
        content: view.result.content.map((block) => ({ type: "text", text: block.text })),
        isError: view.result.is_error,
        timestamp: 0,
      };
    default:
      return null;
  }
}

/** pi's provider-visible messages, converted to the contract request shape. */
function contractMessagesFrom(piMessages) {
  const out = [];
  for (const message of piMessages) {
    if (message.role === "user") {
      const content =
        typeof message.content === "string"
          ? [{ type: "text", text: message.content }]
          : message.content
              .filter((block) => block.type === "text")
              .map((block) => ({ type: "text", text: block.text }));
      out.push({ role: "user", content });
    } else if (message.role === "assistant") {
      out.push({
        role: "assistant",
        content: message.content
          .filter((block) => block.type === "text" || block.type === "toolCall")
          .map((block) =>
            block.type === "text"
              ? { type: "text", text: block.text }
              : {
                  type: "tool_call",
                  tool_call_id: block.id,
                  name: block.name,
                  input: block.arguments ?? {},
                },
          ),
      });
    } else if (message.role === "toolResult") {
      out.push({
        role: "tool_result",
        tool_call_id: message.toolCallId,
        name: message.toolName,
        is_error: message.isError === true,
        content: (message.content ?? [])
          .filter((block) => block.type === "text")
          .map((block) => ({ type: "text", text: block.text })),
      });
    }
  }
  return out;
}

/** One folded kernel round, replayed as the streamed event protocol pi consumes. */
function streamFolded(ctx, llmContext) {
  const stream = new AssistantMessageEventStream();
  (async () => {
    const request = { messages: contractMessagesFrom(llmContext.messages) };
    if (llmContext.systemPrompt) {
      request.system = llmContext.systemPrompt;
    }
    if ((llmContext.tools ?? []).length > 0) {
      request.tools = llmContext.tools.map((tool) => ({
        name: tool.name,
        description: tool.description,
        input_schema: tool.parameters,
      }));
    }
    const view = await ctx.model.stream(request);
    const content = [];
    const base = () => ({
      role: "assistant",
      content,
      api: "aex-loophost",
      provider: "aex-sealed",
      model: view.model,
      usage: usageFromView(view.usage),
      stopReason: "pending",
      timestamp: 0,
    });
    stream.push({ type: "start", partial: base() });
    for (const block of view.content) {
      const index = content.length;
      if (block.type === "text") {
        content.push({ type: "text", text: "" });
        stream.push({ type: "text_start", contentIndex: index, partial: base() });
        content[index].text = block.text;
        stream.push({
          type: "text_delta",
          contentIndex: index,
          delta: block.text,
          partial: base(),
        });
        stream.push({
          type: "text_end",
          contentIndex: index,
          content: block.text,
          partial: base(),
        });
      } else {
        const toolCall = {
          type: "toolCall",
          id: block.tool_call_id,
          name: block.name,
          arguments: block.input,
        };
        content.push(toolCall);
        stream.push({ type: "toolcall_start", contentIndex: index, partial: base() });
        stream.push({ type: "toolcall_end", contentIndex: index, toolCall, partial: base() });
      }
    }
    const stopReason = view.stop_reason === "tool_use" ? "toolUse" : "stop";
    const message = { ...base(), stopReason };
    stream.push({ type: "done", reason: stopReason, message });
    stream.end(message);
  })().catch((error) => {
    const message = {
      role: "assistant",
      content: [],
      api: "aex-loophost",
      provider: "aex-sealed",
      model: ctx.session.model,
      usage: usageFromView(undefined),
      stopReason: "error",
      errorMessage: String(error?.message ?? error),
      timestamp: 0,
    };
    stream.push({ type: "error", reason: "error", error: message.errorMessage, message });
    stream.end(message);
  });
  return stream;
}

/** The sealed grant (delivered on session metadata), presented as pi tools. */
function sealedTools(ctx) {
  const declared = ctx.session.metadata?.tools;
  if (!Array.isArray(declared)) {
    return [];
  }
  return declared.map((tool) => ({
    name: tool.name,
    label: tool.name,
    description: tool.description ?? "",
    parameters: tool.input_schema ?? { type: "object", additionalProperties: true },
    async execute(toolCallId, params) {
      const [result] = await ctx.tools.dispatch([
        { tool_call_id: toolCallId, name: tool.name, input: params ?? {} },
      ]);
      const text = (result?.content ?? []).map((block) => block.text).join("\n");
      if (result?.is_error) {
        throw new Error(text || "tool failed");
      }
      return { content: [{ type: "text", text }], details: result };
    },
  }));
}

// pi conversation memory: a cache exactly as durable as this resident instance, rebuilt from
// the delivered hydration tail on a fresh instance.
let memory = [];

export const { activate } = defineAgentloop({
  onSessionStart(start) {
    memory = start.tail.map(piFromView).filter((message) => message !== null);
  },
  async onMessage(ctx, admitted) {
    const context = {
      systemPrompt: "",
      messages: memory,
      tools: sealedTools(ctx),
    };
    const config = {
      model: piModel(ctx.session.model),
      convertToLlm: (messages) =>
        messages.filter(
          (message) =>
            message.role === "user" ||
            message.role === "assistant" ||
            message.role === "toolResult",
        ),
    };
    const user = { role: "user", content: textOf(admitted.content), timestamp: 0 };
    const streamFn = (_model, llmContext) => streamFolded(ctx, llmContext);
    const finalMessages = await runAgentLoop(
      [user],
      context,
      config,
      () => {},
      undefined,
      streamFn,
    );
    // Robust to either return convention (full conversation or new messages only).
    const seen = new Set(memory);
    for (const message of finalMessages) {
      if (!seen.has(message)) {
        memory.push(message);
      }
    }
    const assistants = finalMessages.filter((message) => message.role === "assistant");
    const errored = assistants.at(-1)?.stopReason === "error" ? assistants.at(-1) : null;
    if (errored) {
      await ctx.turn.fail({ message: errored.errorMessage ?? "the pi loop errored" });
    }
    // Returning finishes the turn.
  },
});
