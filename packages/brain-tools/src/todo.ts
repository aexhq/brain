import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

import { defineTool } from "@aexhq/brain";
import { z } from "zod";

import { workspacePath } from "./path.js";

const item = z.object({ text: z.string().min(1), done: z.boolean().default(false) });

const todo = defineTool({
  module: import.meta.url,
  name: "todo",
  description: "Read or replace the session's portable to-do list.",
  input: z.discriminatedUnion("action", [
    z.object({ action: z.literal("get") }),
    z.object({ action: z.literal("set"), items: z.array(item).max(200) }),
  ]),
  output: z.object({ items: z.array(item) }),
  async execute(input, context) {
    const path = workspacePath(context.workspace, ".brain/todo.json");
    if (input.action === "set") {
      await mkdir(dirname(path), { recursive: true });
      await writeFile(path, `${JSON.stringify(input.items, null, 2)}\n`, "utf8");
      return { items: input.items };
    }
    try {
      return { items: z.array(item).parse(JSON.parse(await readFile(path, "utf8"))) };
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === "ENOENT") return { items: [] };
      throw error;
    }
  },
});

export default todo;
