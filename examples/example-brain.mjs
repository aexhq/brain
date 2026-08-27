import { brain } from "@aexhq/brain";
import { z } from "zod";

export const example = brain((author) => {
  const state = author.state(z.object({ messages: z.array(z.unknown()) }), () => ({ messages: [] }));
  author.on.message((message, turn) => {
    state.messages.push({ role: "user", content: message.content });
    return turn.model({ messages: state.messages });
  });
  author.on.model((completed, turn) => {
    const response = completed.response?.response ?? completed.response ?? {};
    const text = typeof response.text === "string" ? response.text : "";
    state.messages.push({ role: "assistant", content: text });
    return turn.reply(text);
  });
});
