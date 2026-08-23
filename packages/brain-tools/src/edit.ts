import { readFile, writeFile } from "node:fs/promises";

import { tool } from "@aexhq/brain";
import { z } from "zod";

import { workspaceOf, workspacePath } from "./path.js";

const editInput = z.object({ path: z.string().min(1), old_text: z.string().min(1), new_text: z.string() });
const editOutput = z.object({ path: z.string(), replacements: z.literal(1) });

const edit = tool(editInput, async function edit({ path, old_text, new_text }, context) {
    const target = workspacePath(workspaceOf(context), path);
    const content = await readFile(target, "utf8");
    const first = content.indexOf(old_text);
    if (first < 0) throw new Error("old_text was not found");
    if (content.indexOf(old_text, first + old_text.length) >= 0) {
      throw new Error("old_text occurs more than once; provide a more specific match");
    }
    await writeFile(target, `${content.slice(0, first)}${new_text}${content.slice(first + old_text.length)}`, "utf8");
    return { path, replacements: 1 as const };
  })
  .named("edit")
  .describe("Replace one exact occurrence of text in a Environment workspace file.")
  .returns(editOutput)
  .server(import.meta.url);

export default edit;
