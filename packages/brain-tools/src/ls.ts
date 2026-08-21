import { readdir } from "node:fs/promises";

import { tool } from "@aexhq/brain";
import { z } from "zod";

import { workspaceOf, workspacePath } from "./path.js";

const lsInput = z.object({ path: z.string().default("."), limit: z.number().int().positive().max(10_000).default(1_000) });
const lsOutput = z.object({ entries: z.array(z.object({ name: z.string(), kind: z.enum(["file", "directory", "symlink", "other"]) })), truncated: z.boolean() });

const ls = tool(lsInput, async function ls({ path, limit }, context) {
    const values = await readdir(workspacePath(workspaceOf(context), path), { withFileTypes: true });
    values.sort((left, right) => left.name.localeCompare(right.name));
    const kind = (value: (typeof values)[number]): "file" | "directory" | "symlink" | "other" =>
      value.isFile() ? "file" : value.isDirectory() ? "directory" : value.isSymbolicLink() ? "symlink" : "other";
    return {
      entries: values.slice(0, limit).map((value) => ({ name: value.name, kind: kind(value) })),
      truncated: values.length > limit,
    };
  })
  .describe("List entries in a Hand workspace directory.")
  .returns(lsOutput)
  .server(import.meta.url);

export default ls;
