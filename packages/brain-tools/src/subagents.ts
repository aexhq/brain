import { defineIntrinsicTool } from "@aexhq/brain";
import { z } from "zod";

const subagents = defineIntrinsicTool({
  name: "task",
  description: "Delegate a bounded task to a child agent in this session.",
  input: z.object({ description: z.string().min(1), prompt: z.string().min(1) }),
  output: z.object({ agent_id: z.string(), outcome: z.enum(["completed", "failed", "cancelled"]), summary: z.string() }),
  capability: "brain.subagents.v1",
});

export default subagents;
