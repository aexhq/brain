import { agentloop } from "@aexhq/brain";
import { z } from "zod";

export const diagnostic = agentloop((author) => {
  const memory = author.slot("memory", z.object({ turns: z.number().int().nonnegative() }), () => ({ turns: 0 }));
  author.turn(async (turn) => {
    memory.turns += 1;
    await turn.append("note", { turns: memory.turns });
    return turn.done({ turns: memory.turns, message: turn.input.message });
  });
});
