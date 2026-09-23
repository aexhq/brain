import { tool } from "@aexhq/brain";
import { z } from "zod";

export const summarize = tool({
  name: "summarize",
  description: "Summarize a document.",
  input: z.object({ document: z.string() }),
  run: async ({ document }, ctx) => {
    const response = await ctx.model({
      system: "Summarize this document in one sentence.",
      messages: [{ role: "user", content: [{ type: "text", text: document }] }],
    });
    const summary = response.message.content.filter(block => block.type === "text").map(block => block.text).join("\n");
    return ctx.finish({ document, summary }, { content: summary });
  },
});
