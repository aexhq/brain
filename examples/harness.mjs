import { serveHarness } from "@aexhq/brain/harness";

// Start Brain with BRAIN_AGENTLOOPS='{"custom":{"command":"node","args":["examples/harness.mjs"]}}'.
// This small custom implementation makes one model call for each user message.
// Select { name: "custom" } at session creation. Upstream agents live in extension packages.
await serveHarness({
  name: "single-call", stateVersion: "1",
  open() {
    return {
      async run(ctx) {
        if (!ctx.input) return;
        const messages = [...ctx.transcript, { role: "user", content: [
          { type: "text", text: ctx.input.message }, ...(ctx.input.media ?? []),
        ] }];
        const reply = await ctx.model({ messages, tools: [] });
        await ctx.setTranscript([...messages, reply.message]);
      },
      close() {},
    };
  },
});
