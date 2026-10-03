import { defineAgentloop } from "@aexhq/brain/agentloop";

export const turn = defineAgentloop(async ctx => {
  if (!ctx.input) return;
  const messages = ctx.transcript;
  messages.push({
    role: "user",
    content: [{ type: "text", text: ctx.input.message }, ...(ctx.input.media ?? [])],
  });
  await ctx.setTranscript(messages);
  const reply = await ctx.model({ messages, system: ctx.system, tools: [] });
  messages.push(reply.message);
  await ctx.setTranscript(messages);
});
