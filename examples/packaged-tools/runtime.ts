import { readFile } from "node:fs/promises";
import { tool } from "@aexhq/brain";
import { z } from "zod";

export const readText = tool({
  name: "read_text",
  description: "Read a text file.",
  input: z.object({ path: z.string() }),
  run: async ({ path }, ctx) => ctx.finish(await readFile(path, "utf8")),
});
