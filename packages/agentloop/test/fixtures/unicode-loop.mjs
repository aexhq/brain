import { defineAgentloop } from "@aexhq/agentloop";

const identifier = /^\p{L}+$/u;

export const { activate } = defineAgentloop({
  async onMessage(ctx, message) {
    const text = message.content[0]?.type === "text" ? message.content[0].text : "";
    await ctx.turn.finish({ matched: identifier.test(text) });
  },
});
