import { defineAgentloop } from "@aexhq/agentloop";

export default defineAgentloop({
  step(input) {
    const activations = Number(input.context.state?.activations ?? 0) + 1;
    return {
      context: {
        ...input.context,
        state: { activations, observation: input.observation.type },
      },
      decision: {
        type: "finish",
        result: { activations, observation: input.observation.type },
      },
    };
  },
});
