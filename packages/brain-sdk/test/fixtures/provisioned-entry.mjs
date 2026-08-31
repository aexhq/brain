// Build fixture for the provisioned-tool artifact golden test.
import { tool } from "@aexhq/brain";
import { z } from "zod";

export const bash = tool({
  description: "Run a shell command in the session workspace.",
  input: z.object({ command: z.string(), timeout_ms: z.number().int().optional() }),
  output: z.object({ exit_code: z.number(), stdout: z.string(), stderr: z.string() }),
  requires: ["exec"],
  bindings: { API_BASE: z.string() },
}, (author) => {
  author.run(async (input, context) => {
    const result = await context.exec.run(`${input.command} # ${context.bindings.API_BASE}`, { timeoutMs: input.timeout_ms ?? 999_999_999 });
    return { exit_code: result.exitCode, stdout: result.stdout, stderr: result.stderr };
  });
});

export const idle = tool({
  description: "Declares fs but never touches it.",
  input: z.object({}),
  requires: ["fs"],
}, (author) => {
  author.run(async () => null);
});
