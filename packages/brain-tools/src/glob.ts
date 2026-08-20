import { glob as fsGlob } from "node:fs/promises";
import { relative } from "node:path";

import { defineTool } from "@aexhq/brain";
import { z } from "zod";

const glob = defineTool({
  module: import.meta.url,
  name: "glob",
  description: "List Hand workspace paths matching a glob pattern.",
  input: z.object({ pattern: z.string().min(1), limit: z.number().int().positive().max(10_000).default(1_000) }),
  output: z.object({ paths: z.array(z.string()), truncated: z.boolean() }),
  async execute({ pattern, limit }, context) {
    const paths: string[] = [];
    for await (const entry of fsGlob(pattern, { cwd: context.workspace, withFileTypes: true })) {
      paths.push(relative(context.workspace, entry.parentPath === context.workspace ? entry.name : `${entry.parentPath}/${entry.name}`).replaceAll("\\", "/"));
      if (paths.length > limit) break;
    }
    paths.sort();
    return { paths: paths.slice(0, limit), truncated: paths.length > limit };
  },
});

export default glob;
