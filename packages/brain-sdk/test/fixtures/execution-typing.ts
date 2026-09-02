/**
 * Compile-time fixture for the built tool context: it carries the typed binding
 * values and the invocation plumbing, and nothing about resources — a program
 * reaches those through the platform. Compiled by test/execution.test.mjs with
 * `tsc --noEmit`; every `@ts-expect-error` line asserts a required error.
 */
import { environment, tool } from "../../dist/index.js";
import { z } from "zod";

export const declared = tool({
  description: "Uses its bindings and the platform.",
  input: z.object({ command: z.string() }),
  output: z.object({ exit_code: z.number() }),
  needs: ["process", "fs"],
  bindings: { API_BASE: z.string() },
}, (author) => {
  author.run(async (input, context) => {
    const base: string = context.bindings.API_BASE;
    void base;
    void input.command;
    void context.signal;
    void context.deadline;
    void context.callId;
    context.progress({ stage: "running" });
    // @ts-expect-error there are no resource handles; use node:child_process
    void context.exec;
    // @ts-expect-error there are no resource handles; use node:fs
    void context.fs;
    // @ts-expect-error MISSING is not a declared binding
    void context.bindings.MISSING;
    return { exit_code: 0 };
  });
});

export const script = tool.shell({
  description: "One command.",
  input: z.object({ command: z.string() }),
  needs: ["process"],
  script: "$command",
});

export const call = tool.http({
  description: "One request.",
  input: z.object({ query: z.string() }),
  request: { method: "POST", url: "https://service.internal/search" },
});

export const box = environment({ options: z.object({ image: z.string() }), resources: { fs: { root: "/workspace" }, process: {}, "bin:ffmpeg": { version: "7" } } }, (author) => {
  const instance = author.open(async ({ options }) => ({ image: options.image }));
  instance.execute.esm();
  instance.execute.shell(async (context, program) => ({ exit_code: 0, stdout: `${context.instance.image}:${program}`, stderr: "" }));
  instance.execute.http();
  instance.close(async () => undefined);
  return {};
});
