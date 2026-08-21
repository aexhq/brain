import { defineAgentloop } from "@aexhq/agentloop";

export const { activate } = defineAgentloop({
  async onMessage(ctx, message) {
    const round = await ctx.model.stream({
      messages: [{ role: "user", content: message.content }],
    });
    await ctx.journal.append([
      { kind: "event", name: "probe.round", data: { stop: round.stop_reason } },
    ]);
    await ctx.turn.finish();
  },
});
