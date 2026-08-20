import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

import { defineTool } from "@aexhq/brain";
import { z } from "zod";

import { workspacePath } from "./path.js";

const write = defineTool({
  module: import.meta.url,
  name: "write",
  description: "Write UTF-8 text to a file in the Hand workspace, creating parent directories.",
  input: z.object({ path: z.string().min(1), content: z.string() }),
  output: z.object({ path: z.string(), bytes: z.number().int().nonnegative() }),
  async execute({ path, content }, context) {
    const target = workspacePath(context.workspace, path);
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, content, "utf8");
    return { path, bytes: Buffer.byteLength(content) };
  },
});

export default write;
