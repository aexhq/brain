import { agentloop } from "@aexhq/brain";
import { z } from "zod";

const text = (message) =>
  message.content
    .filter((block) => block.type === "text")
    .map((block) => block.text)
    .join("");

// A full coding turn: model, tools, model again, until the model answers in prose.
export const coder = agentloop((author) => {
  const memory = author.slot("memory", z.object({ turns: z.number().int() }), () => ({ turns: 0 }));

  author.turn(async (turn) => {
    memory.turns += 1;
    turn.transcript.push({ role: "user", content: [{ type: "text", text: turn.input.message }] });
    for (;;) {
      const { message } = await turn.model({ messages: turn.transcript });
      turn.transcript.push(message);
      const calls = message.content
        .filter((block) => block.type === "tool_use")
        .map((block) => ({ callId: block.id, name: block.name, input: block.input }));
      if (calls.length === 0) {
        await turn.reply(text(message));
        return turn.done({ turns: memory.turns });
      }
      // One dispatch: Brain runs every call in parallel and reports back once, in order.
      const results = await turn.dispatch(calls);
      turn.transcript.push({
        role: "user",
        content: results.map((result) => ({
          type: "tool_result",
          tool_use_id: result.callId,
          content: result.output,
          is_error: result.isError,
        })),
      });
    }
  });
});
