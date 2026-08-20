import { spawn } from "node:child_process";

import { defineTool } from "@aexhq/brain";
import { z } from "zod";

const MAX_CAPTURE_BYTES = 1024 * 1024;

const bash = defineTool({
  module: import.meta.url,
  name: "bash",
  description: "Run a Bash command in the session Hand workspace.",
  input: z.object({
    command: z.string().min(1),
    cwd: z.string().optional(),
    timeout_ms: z.number().int().positive().optional(),
  }),
  output: z.object({
    stdout: z.string(),
    stderr: z.string(),
    exit_code: z.number().int().nullable(),
    signal: z.string().nullable(),
    truncated: z.boolean(),
  }),
  async execute(input, context) {
    const deadline = input.timeout_ms === undefined
      ? context.deadlineMs
      : Math.min(context.deadlineMs, Date.now() + input.timeout_ms);
    return await new Promise((resolve, reject) => {
      const child = spawn("/bin/bash", ["-lc", input.command], {
        cwd: input.cwd ?? context.workspace,
        env: process.env,
        detached: false,
        signal: context.signal,
        stdio: ["ignore", "pipe", "pipe"],
      });
      let stdout = Buffer.alloc(0);
      let stderr = Buffer.alloc(0);
      let truncated = false;
      const add = (target: "stdout" | "stderr", chunk: Buffer): void => {
        const current = target === "stdout" ? stdout : stderr;
        const remaining = Math.max(0, MAX_CAPTURE_BYTES - current.byteLength);
        if (chunk.byteLength > remaining) truncated = true;
        const next = Buffer.concat([current, chunk.subarray(0, remaining)]);
        if (target === "stdout") stdout = next;
        else stderr = next;
      };
      child.stdout.on("data", (chunk: Buffer) => add("stdout", chunk));
      child.stderr.on("data", (chunk: Buffer) => add("stderr", chunk));
      child.once("error", reject);
      child.once("close", (code, signal) => {
        clearTimeout(timer);
        resolve({
          stdout: stdout.toString("utf8"),
          stderr: stderr.toString("utf8"),
          exit_code: code,
          signal,
          truncated,
        });
      });
      const delay = Math.max(1, deadline - Date.now());
      const timer = setTimeout(() => child.kill("SIGTERM"), delay);
      timer.unref();
    });
  },
});

export default bash;
