import { agentloop } from "@aexhq/brain";
import { z } from "zod";

export const diagnostic = agentloop((author) => {
  const state = author.state(z.object({ activations: z.number().int().nonnegative() }), () => ({ activations: 0 }));
  author.on.message((message, turn) => {
    state.activations += 1;
    return turn.done({ activations: state.activations, observation: message.type });
  });
});
