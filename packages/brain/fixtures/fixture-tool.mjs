export default {
  kind: "brain.tool",
  name: "fixture",
  description: "Builder fixture.",
  input: {},
  output: {},
  requiredEnv: [],
  execution: "hand",
  executor: { kind: "hand" },
  execute: async (input) => input,
};
