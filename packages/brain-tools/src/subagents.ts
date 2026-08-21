import { officialTool } from "@aexhq/brain/internal";
import { z } from "zod";

const childId = z.string().min(1).max(128).regex(/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/);
const message = z.string().min(1).max(192 * 1024);

const subagents = officialTool({
  name: "subagents",
  description: "Create and explicitly interact with durable direct child sessions.",
  input: z.discriminatedUnion("action", [
    z.object({
      action: z.literal("spawn_agent"),
      task_name: z.string().min(1).max(128),
      message,
      fork_turns: z.union([z.literal("all"), z.literal("none"), z.string().max(10).regex(/^[1-9][0-9]*$/)]).optional(),
    }),
    z.object({ action: z.literal("send_message"), child_id: childId, message }),
    z.object({ action: z.literal("follow_up"), child_id: childId, message }),
    z.object({ action: z.literal("wait"), child_id: childId, timeout_ms: z.number().int().nonnegative().max(300_000).optional() }),
    z.object({ action: z.literal("peek"), child_id: childId }),
    z.object({ action: z.literal("list_children"), cursor: z.string().max(4096).optional(), limit: z.number().int().positive().max(100).optional() }),
    z.object({ action: z.literal("interrupt_agent"), child_id: childId }),
    z.object({ action: z.literal("end_agent"), child_id: childId }),
  ]),
  output: z.unknown(),
  capability: "brain.subagents",
});

export default subagents;
