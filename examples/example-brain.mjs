import { brain } from "@aexhq/brain";
import { z } from "zod";

export const example = brain((author) => {
  const state = author.state(z.object({ messages: z.array(z.unknown()) }), () => ({ messages: [] }));
  author.on.message((message, turn) => {
    const text = typeof message.content === "string" ? message.content : JSON.stringify(message.content);
    state.messages.push({ role: "user", content: [{ type: "text", text }] });
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
