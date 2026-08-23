import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

import { tool } from "@aexhq/brain";
import { z } from "zod";

import { workspaceOf, workspacePath } from "./path.js";

const writeInput = z.object({ path: z.string().min(1), content: z.string() });
const writeOutput = z.object({ path: z.string(), bytes: z.number().int().nonnegative() });

const write = tool(writeInput, async function write({ path, content }, context) {
    const target = workspacePath(workspaceOf(context), path);
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, content, "utf8");
    return { path, bytes: Buffer.byteLength(content) };
  })
  .named("write")
  .describe("Write UTF-8 text to a file in the Environment workspace, creating parent directories.")
  .returns(writeOutput)
  .server(import.meta.url);

export default write;
