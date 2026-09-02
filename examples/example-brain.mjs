import { agentloop } from "@aexhq/brain";
import { z } from "zod";

// The loop owns what the model is told: the system prompt comes in as an option, and
// every tool the session was created with is offered on each call.
export const example = agentloop({ options: z.object({ system: z.string().default("") }) }, (author) => {
  const state = author.state(z.object({ messages: z.array(z.unknown()) }), () => ({ messages: [] }));
  author.on.message(({ input }, turn) => {
    state.messages.push({ role: "user", content: [{ type: "text", text: input.message }] });
    return turn.model({ system: author.options.system, tools: author.tools.map((tool) => tool.name), messages: state.messages });
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
