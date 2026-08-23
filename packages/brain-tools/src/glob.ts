import { glob as fsGlob } from "node:fs/promises";
import { relative } from "node:path";

import { tool } from "@aexhq/brain";
import { z } from "zod";

import { workspaceOf } from "./path.js";

const globInput = z.object({ pattern: z.string().min(1), limit: z.number().int().positive().max(10_000).default(1_000) });
const globOutput = z.object({ paths: z.array(z.string()), truncated: z.boolean() });

const glob = tool(globInput, async function glob({ pattern, limit }, context) {
    const paths: string[] = [];
    const workspace = workspaceOf(context);
    for await (const entry of fsGlob(pattern, { cwd: workspace, withFileTypes: true })) {
      paths.push(relative(workspace, entry.parentPath === workspace ? entry.name : `${entry.parentPath}/${entry.name}`).replaceAll("\\", "/"));
      if (paths.length > limit) break;
    }
    paths.sort();
    return { paths: paths.slice(0, limit), truncated: paths.length > limit };
  })
  .named("glob")
  .describe("List Environment workspace paths matching a glob pattern.")
  .returns(globOutput)
  .server(import.meta.url);

export default glob;
