import { open } from "node:fs/promises";

import { defineTool } from "@aexhq/brain";
import { z } from "zod";

import { workspacePath } from "./path.js";

const read = defineTool({
  module: import.meta.url,
  name: "read",
  description: "Read UTF-8 text from a file in the Hand workspace.",
  input: z.object({
    path: z.string().min(1),
    offset: z.number().int().nonnegative().default(0),
    limit: z.number().int().positive().max(1024 * 1024).default(256 * 1024),
  }),
  output: z.object({ content: z.string(), bytes: z.number().int().nonnegative(), truncated: z.boolean() }),
  async execute({ path, offset, limit }, context) {
    const file = await open(workspacePath(context.workspace, path), "r");
    try {
      const buffer = Buffer.alloc(limit + 1);
      const { bytesRead } = await file.read(buffer, 0, buffer.byteLength, offset);
      const data = buffer.subarray(0, Math.min(bytesRead, limit));
      if (data.includes(0)) throw new Error(`${path} is binary`);
      return { content: data.toString("utf8"), bytes: data.byteLength, truncated: bytesRead > limit };
    } finally {
      await file.close();
    }
  },
});

export default read;
