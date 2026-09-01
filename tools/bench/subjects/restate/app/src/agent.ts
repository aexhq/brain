// The agent-session service this benchmark runs on Restate.
//
// This is harness code on Restate's substrate — Restate ships no agent — and it
// deviates from their published "Durable Sessions" pattern in nothing but the model's
// base URL: a Virtual Object keyed by session id, history in object state through
// ctx.get/set, the model call wrapped in their own durableCalls middleware so every
// LLM call is journaled and replayed rather than re-issued. The manifest says all of
// this, because a number measured on a harness has to say who wrote the harness.
import * as restate from "@restatedev/restate-sdk";
import { createOpenAI } from "@ai-sdk/openai";
import { generateText, wrapLanguageModel, type ModelMessage } from "ai";
import { durableCalls } from "@restatedev/vercel-ai-middleware";

const provider = createOpenAI({
  baseURL: process.env.BENCH_MODEL_BASE_URL!,
  apiKey: "bench",
});

const agentSession = restate.object({
  name: "AgentSession",
  handlers: {
    send: async (ctx: restate.ObjectContext, request: { message: string }) => {
      const model = wrapLanguageModel({
        // .chat pins the chat-completions dialect the scripted provider speaks.
        model: provider.chat("gpt-4o-mini"),
        middleware: durableCalls(ctx, { maxRetryAttempts: 3 }),
      });
      const history = (await ctx.get<ModelMessage[]>("messages")) ?? [];
      history.push({ role: "user", content: request.message });
      const result = await generateText({ model, messages: history });
      ctx.set("messages", [...history, ...result.response.messages]);
      return result.text;
    },
  },
});

restate.serve({ services: [agentSession], port: 9080 });
