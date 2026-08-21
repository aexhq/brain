// The official codex-style loop: a TypeScript semantic port of codex-rs's loop policies
// (aex-research docs/codex-loop-semantics.md — codex's loop is Rust and cannot be embedded).
// v1 ports: the environment preamble, strictly sequential tool execution, loop-owned
// conversation memory, and a summary mark per turn.
import { defineAgentloop } from "@aexhq/agentloop";

const textOf = (content) =>
  content
    .filter((block) => block.type === "text")
    .map((block) => block.text)
    .join("");

/** Rebuild conversation messages from the hydration tail views. */
function messageFromView(view) {
  switch (view.type) {
    case "user_message":
      return { role: "user", content: view.content };
    case "assistant_message":
      return { role: "assistant", content: view.message.content };
    case "tool_result":
      return {
        role: "tool_result",
        tool_call_id: view.result.tool_call_id,
        name: view.result.name,
        is_error: view.result.is_error,
        content: view.result.content,
      };
    default:
      return null;
  }
}

function environmentPreamble(ctx) {
  const tools = Array.isArray(ctx.session.metadata?.tools)
    ? ctx.session.metadata.tools.map((tool) => tool.name).join(", ")
    : "none";
  return [
    "You are a precise engineering agent.",
    "<environment_context>",
    `session: ${ctx.session.session_id}`,
    `model: ${ctx.session.model}`,
    `sealed tools: ${tools}`,
    "</environment_context>",
  ].join("\n");
}

// Loop-owned conversation memory: rebuilt from the delivered hydration on a fresh instance
// (summary mark first, then the tail), extended per turn.
let memory = [];

export const { activate } = defineAgentloop({
  onSessionStart(start) {
    memory = [];
    if (start.latest_mark && typeof start.latest_mark.data.summary === "string") {
      memory.push({
        role: "user",
        content: [
          {
            type: "text",
            text: `<conversation_summary>\n${start.latest_mark.data.summary}\n</conversation_summary>`,
          },
        ],
      });
    }
    for (const view of start.tail) {
      const message = messageFromView(view);
      if (message !== null) {
        memory.push(message);
      }
    }
  },
  async onMessage(ctx, admitted) {
    const presented = Array.isArray(ctx.session.metadata?.tools)
      ? ctx.session.metadata.tools.map((tool) => ({
          name: tool.name,
          description: tool.description,
          input_schema: tool.input_schema,
        }))
      : [];
    memory.push({ role: "user", content: admitted.content });
    let finalText = "";
    for (;;) {
      const request = {
        system: environmentPreamble(ctx),
        messages: memory.slice(),
      };
      if (presented.length > 0) {
        request.tools = presented;
      }
      const round = await ctx.model.stream(request);
      memory.push({ role: "assistant", content: round.content });
      const calls = round.content.filter((block) => block.type === "tool_call");
      if (calls.length === 0) {
        finalText = textOf(round.content);
        break;
      }
      // codex semantics: strictly sequential execution — one dispatch per call, in order,
      // each result folded into the conversation before the next call runs.
      for (const call of calls) {
        const [result] = await ctx.tools.dispatch([
          { tool_call_id: call.tool_call_id, name: call.name, input: call.input },
        ]);
        memory.push({
          role: "tool_result",
          tool_call_id: result.tool_call_id,
          name: result.name,
          is_error: result.is_error,
          content: result.content,
        });
      }
    }
    // The summary mark: hydration after eviction starts from this plus the tail.
    await ctx.journal.append([
      {
        kind: "mark",
        covers_through_seq: admitted.seq,
        data: { summary: finalText.slice(0, 4000) },
      },
    ]);
    // Returning finishes the turn.
  },
});
