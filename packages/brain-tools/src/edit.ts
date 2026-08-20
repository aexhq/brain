import { readFile, writeFile } from "node:fs/promises";

import { defineTool } from "@aexhq/brain";
import { z } from "zod";

import { workspacePath } from "./path.js";

const edit = defineTool({
  module: import.meta.url,
  name: "edit",
  description: "Replace one exact occurrence of text in a Hand workspace file.",
  input: z.object({ path: z.string().min(1), old_text: z.string().min(1), new_text: z.string() }),
  output: z.object({ path: z.string(), replacements: z.literal(1) }),
  async execute({ path, old_text, new_text }, context) {
    const target = workspacePath(context.workspace, path);
    const content = await readFile(target, "utf8");
    const first = content.indexOf(old_text);
    if (first < 0) throw new Error("old_text was not found");
    if (content.indexOf(old_text, first + old_text.length) >= 0) {
      throw new Error("old_text occurs more than once; provide a more specific match");
    }
    await writeFile(target, `${content.slice(0, first)}${new_text}${content.slice(first + old_text.length)}`, "utf8");
    return { path, replacements: 1 as const };
  },
});

export default edit;
