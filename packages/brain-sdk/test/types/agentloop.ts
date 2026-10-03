import { defineAgentloop } from "@aexhq/brain/agentloop";

export const turn = defineAgentloop(async ctx => {
  const count = await ctx.kv.get<number>("turns") ?? 0;
  await ctx.kv.set("turns", count + 1);
  if (!ctx.input) return;
  const messages = ctx.transcript;
  messages.push({ role: "user", content: [{ type: "text", text: ctx.input.message }] });
  const reply = await ctx.model({ messages, tools: [] });
  await ctx.setTranscript([...messages, reply.message]);
  const result = await ctx.callTool({ call_id: "lookup-1", name: "lookup", environment: "app", input: {} });
  const finished: boolean = result.finished;
  return { finished, count };
});

defineAgentloop(async ctx => {
  // @ts-expect-error A background activation has no user input.
  ctx.input.message;
  // @ts-expect-error Tool calls require an explicit authorized placement.
  await ctx.callTool({ call_id: "lookup-1", name: "lookup", input: {} });
});
