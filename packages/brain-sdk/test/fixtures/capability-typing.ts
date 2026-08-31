/**
 * Compile-time fixture for the provisioned tool context: the run context is
 * derived from `requires` (an undeclared capability must not type-check) and
 * from the `bindings` schemas. Compiled by test/capability.test.mjs with
 * `tsc --noEmit`; every `@ts-expect-error` line asserts a required error.
 */
import { tool } from "../../dist/index.js";
import { z } from "zod";

export const declared = tool({
  description: "Uses only what it declares.",
  input: z.object({ command: z.string() }),
  output: z.object({ exit_code: z.number() }),
  requires: ["exec", "fs"],
  bindings: { API_BASE: z.string() },
}, (author) => {
  author.run(async (input, context) => {
    const result = await context.exec.run(input.command, { timeoutMs: 1_000 });
    await context.fs.write("out.txt", result.stdout);
    const base: string = context.bindings.API_BASE;
    void base;
    void context.signal;
    void context.deadline;
    void context.callId;
    // @ts-expect-error net is not declared in requires
    void context.net;
    // @ts-expect-error page is not declared in requires
    void context.page;
    // @ts-expect-error MISSING is not a declared binding
    void context.bindings.MISSING;
    return { exit_code: result.exitCode };
  });
});

export const undeclared = tool({
  description: "Declares nothing.",
  input: z.object({}),
}, (author) => {
  author.run(async (_input, context) => {
    // @ts-expect-error exec is not declared in requires
    void context.exec;
    return null;
  });
});
