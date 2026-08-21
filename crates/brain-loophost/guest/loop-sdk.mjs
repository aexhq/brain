// An SDK-authored probe loop, exactly as a customer would write one: the e2e proves the
// published authoring surface produces a working guest against the real kernel.
import { defineAgentloop } from "@aexhq/agentloop";

export const { activate } = defineAgentloop({
  async onMessage(ctx, message) {
    const kv = await ctx.kv.get(["n"]);
    const n = (typeof kv.n === "number" ? kv.n : 0) + 1;
    await ctx.kv.set({ n });
    const round = await ctx.model.stream({
      system: "you are the sdk probe",
      messages: [{ role: "user", content: message.content }],
    });
    const text = round.content
      .filter((block) => block.type === "text")
      .map((block) => block.text)
      .join("");
    await ctx.journal.append([
      { kind: "event", name: "sdk.turn", data: { n, text, resumed: ctx.start?.resumed ?? null } },
    ]);
    await ctx.turn.finish({ n });
  },
});
