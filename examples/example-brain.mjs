import { agentloop } from "@aexhq/brain";

export const example = agentloop((author) => {
  author.turn(async (turn) => {
    turn.transcript.push({ role: "user", content: [{ type: "text", text: turn.input.message }] });
    // The session's system prompt and tools apply unless the call says otherwise.
    const { message } = await turn.model({ messages: turn.transcript });
    turn.transcript.push(message);
    const text = message.content
      .filter((block) => block.type === "text")
      .map((block) => block.text)
      .join("");
    await turn.reply(text);
    return turn.done();
  });
});
