import { agentloop } from "@aexhq/brain";
import { z } from "zod";

export const example = agentloop((author) => {
  const state = author.state(z.object({ messages: z.array(z.unknown()) }), () => ({ messages: [] }));
  author.on.message(({ input }, turn) => {
    state.messages.push({ role: "user", content: [{ type: "text", text: input.message }] });
    // The session's system prompt and tools apply unless the call says otherwise.
    return turn.model({ messages: state.messages });
  });
  author.on.model((completed, turn) => {
    const { message } = completed.response;
    state.messages.push(message);
    const text = message.content
      .filter((block) => block.type === "text")
      .map((block) => block.text)
      .join("");
    return turn.reply(text);
  });
});
