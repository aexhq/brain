import { serveHarness } from "@aexhq/brain/harness";

// Start Brain with BRAIN_HARNESS='{"command":"node","args":["examples/harness.mjs"]}'.
// This small custom implementation makes one model call for each user message.
// Use brain-harness-pi, brain-harness-codex, or brain-harness-opencode for an upstream agent.
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
