// The VoltAgent server this benchmark measures.
//
// One agent at VoltAgent's defaults — in-memory storage, because that is what the
// framework ships — its model pointed at the scripted provider through the launch
// environment. The provider goes through @ai-sdk/openai-compatible's chatModel so the
// requests land on the fixture's /chat/completions rather than the Responses API that
// plain `openai(...)` would call. No VoltOps keys are set, so nothing phones home.
import { Agent, VoltAgent } from "@voltagent/core";
import { honoServer } from "@voltagent/server-hono";
import { createOpenAICompatible } from "@ai-sdk/openai-compatible";

const scripted = createOpenAICompatible({
  name: "scripted",
  baseURL: process.env.BENCH_MODEL_BASE_URL!,
  apiKey: "bench",
});

const agent = new Agent({
  name: "bench-agent",
  instructions: "You are a benchmark assistant.",
  model: scripted.chatModel("gpt-4o-mini"),
});

new VoltAgent({
  agents: { agent },
  server: honoServer({
    port: Number(process.env.BENCH_PORT),
    hostname: "127.0.0.1",
    configureApp: (app) => {
      app.get("/health", (c) => c.json({ status: "ok" }));
    },
  }),
});
