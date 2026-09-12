import { tool } from "@aexhq/brain";
import { z } from "zod";

const records = new Map([["1", { name: "Ada" }]]);
export const lookupRecord = tool({
  name: "lookup_record",
  description: "Look up a record by id.",
  input: z.object({ id: z.string() }),
  output: z.object({ name: z.string() }),
  run: ({ id }) => records.get(id) ?? {
    status: "error",
    error: { code: "not_found", message: "Record not found", retryable: false, details: { id } },
  },
});
