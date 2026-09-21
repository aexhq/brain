import { z } from "zod";
import { tool, type Outcome } from "../../dist/index.js";

const failure: Outcome<number> = {
  status: "error", error: { code: "denied", message: "Denied", retryable: false, details: { scope: "write" } },
};
tool({ name: "raw", description: "Raw", input: z.object({}), output: z.number(), run: () => 1 });
tool({ name: "known", description: "Known", input: z.object({}), output: z.number(), run: () => failure });
tool({ name: "unknown", description: "Unknown", input: z.object({}), output: z.number(), run: async () => ({ status: "unknown", message: "Connection lost" }) });
tool({ name: "success", description: "Success", input: z.object({}), output: z.string().transform(Number), run: () => ({ status: "ok", value: "1" }) });
tool({ name: "finish", description: "Finish", input: z.object({}), output: z.number(), run: (_, context) => context.finish(1) });
tool({ name: "transformed_finish", description: "Finish", input: z.object({}), output: z.string().transform(Number), run: (_, context) => context.finish("1") });
tool({ name: "background", description: "Background", input: z.object({}), output: z.number(), run: async (_, context) => {
  await context.emitResult(1);
  await context.finish();
} });
// @ts-expect-error Finish validates the output schema's input.
tool({ name: "wrong_finish", description: "Wrong", input: z.object({}), output: z.number(), run: (_, context) => context.finish("1") });
// @ts-expect-error Emitted results validate the output schema's input.
tool({ name: "wrong_emit", description: "Wrong", input: z.object({}), output: z.number(), run: async (_, context) => { await context.emitResult("1"); } });
// @ts-expect-error A successful Outcome must match the output schema's input.
tool({ name: "wrong", description: "Wrong", input: z.object({}), output: z.number(), run: () => ({ status: "ok", value: "1" }) });
