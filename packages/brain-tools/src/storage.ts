import { officialTool } from "@aexhq/brain/internal";
import { z } from "zod";

const key = z.string().min(1).max(1024);
const path = z.string().min(1).max(4096);
const generation = z.string().min(1).max(128).regex(/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/);

const storage = officialTool({
  name: "storage",
  description: "Explicitly save, load, or list durable files for this session.",
  input: z.discriminatedUnion("action", [
    z.object({
      action: z.literal("save"),
      key,
      source: z.discriminatedUnion("kind", [
        z.object({ kind: z.literal("sandbox_path"), path, generation }),
        z.object({ kind: z.literal("inline_text"), text: z.string().max(94_208) }),
      ]),
      overwrite: z.boolean().optional(),
    }),
    z.object({
      action: z.literal("load"),
      key,
      path,
      generation,
      overwrite: z.boolean().optional(),
    }),
    z.object({
      action: z.literal("list"),
      prefix: z.string().max(1024).optional(),
      cursor: z.string().max(4096).optional(),
      limit: z.number().int().positive().max(100).optional(),
    }),
  ]),
  capability: "brain.storage",
});

export default storage;
