import { spawn } from "node:child_process";

import { tool } from "@aexhq/brain";
import { z } from "zod";

const grepInput = z.object({ pattern: z.string().min(1), path: z.string().default("."), limit: z.number().int().positive().max(10_000).default(1_000) });
const grepOutput = z.object({ matches: z.array(z.string()), truncated: z.boolean() });

const grep = tool(grepInput, async function grep({ pattern, path, limit }, context) {
    return await new Promise((resolve, reject) => {
      const child = spawn("rg", ["--line-number", "--no-heading", "--color", "never", "--", pattern, path], {
        cwd: context.workspace,
        signal: context.signal,
        stdio: ["ignore", "pipe", "pipe"],
      });
      let output = "";
      let error = "";
      child.stdout.setEncoding("utf8");
      child.stderr.setEncoding("utf8");
      child.stdout.on("data", (chunk: string) => { if (output.length < 2 * 1024 * 1024) output += chunk; });
      child.stderr.on("data", (chunk: string) => { if (error.length < 64 * 1024) error += chunk; });
      child.once("error", reject);
      child.once("close", (code) => {
        if (code !== 0 && code !== 1) return reject(new Error(error.trim() || `ripgrep exited ${code}`));
        const lines = output.split(/\r?\n/u).filter(Boolean);
        resolve({ matches: lines.slice(0, limit), truncated: lines.length > limit });
      });
    });
  })
  .named("grep")
  .describe("Search text files in the Environment workspace with ripgrep.")
  .returns(grepOutput)
  .server(import.meta.url);

export default grep;
