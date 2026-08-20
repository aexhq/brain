import { readdir } from "node:fs/promises";

import { defineTool } from "@aexhq/brain";
import { z } from "zod";

import { workspacePath } from "./path.js";

const ls = defineTool({
  module: import.meta.url,
  name: "ls",
  description: "List entries in a Hand workspace directory.",
  input: z.object({ path: z.string().default("."), limit: z.number().int().positive().max(10_000).default(1_000) }),
  output: z.object({ entries: z.array(z.object({ name: z.string(), kind: z.enum(["file", "directory", "symlink", "other"]) })), truncated: z.boolean() }),
  async execute({ path, limit }, context) {
    const values = await readdir(workspacePath(context.workspace, path), { withFileTypes: true });
    values.sort((left, right) => left.name.localeCompare(right.name));
    const kind = (value: (typeof values)[number]): "file" | "directory" | "symlink" | "other" =>
      value.isFile() ? "file" : value.isDirectory() ? "directory" : value.isSymbolicLink() ? "symlink" : "other";
    return {
      entries: values.slice(0, limit).map((value) => ({ name: value.name, kind: kind(value) })),
      truncated: values.length > limit,
    };
  },
});

export default ls;
