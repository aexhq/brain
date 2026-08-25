import { officialTool } from "@aexhq/brain/internal";
import { z } from "zod";

const identifier = z.string().min(1).max(128).regex(/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/);
const sandboxId = identifier;
const generation = identifier;
const path = z.string().min(1).max(4096);
const expression = z.string().min(1).max(4096);

const sandbox = officialTool({
  name: "sandbox",
  description: "Create and explicitly operate additional isolated sandboxes.",
  input: z.discriminatedUnion("action", [
    z.object({ action: z.literal("create") }),
    z.object({ action: z.literal("list"), cursor: z.string().max(4096).optional(), limit: z.number().int().positive().max(100).optional() }),
    z.object({ action: z.literal("status"), sandbox_id: sandboxId }),
    z.object({ action: z.literal("exec"), sandbox_id: sandboxId, generation, command: z.string().min(1).max(131_072), cwd: path.optional(), interactive: z.boolean().optional() }),
    z.object({ action: z.literal("write_stdin"), sandbox_id: sandboxId, generation, execution_id: identifier, text: z.string().max(4096).optional(), eof: z.boolean().optional() }),
    z.object({ action: z.literal("list_files"), sandbox_id: sandboxId, generation, path, cursor: z.string().max(4096).optional(), limit: z.number().int().positive().max(100).optional() }),
    z.object({ action: z.literal("stat_file"), sandbox_id: sandboxId, generation, path }),
    z.object({ action: z.literal("read_file"), sandbox_id: sandboxId, generation, path }),
    z.object({ action: z.literal("write_file"), sandbox_id: sandboxId, generation, path, text: z.string().max(94_208), overwrite: z.boolean().optional() }),
    z.object({ action: z.literal("find_files"), sandbox_id: sandboxId, generation, path, glob: expression, cursor: z.string().max(4096).optional(), limit: z.number().int().positive().max(100).optional() }),
    z.object({ action: z.literal("grep_files"), sandbox_id: sandboxId, generation, path, query: expression, cursor: z.string().max(4096).optional(), limit: z.number().int().positive().max(100).optional() }),
    z.object({ action: z.literal("load"), sandbox_id: sandboxId, generation, key: z.string().min(1).max(1024), path, overwrite: z.boolean().optional() }),
    z.object({ action: z.literal("save"), sandbox_id: sandboxId, generation, path, key: z.string().min(1).max(1024), overwrite: z.boolean().optional() }),
    z.object({ action: z.literal("terminate"), sandbox_id: sandboxId }),
  ]),
  capability: "brain.sandbox",
});

export default sandbox;
