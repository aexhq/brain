import escapeStringRegexp from "escape-string-regexp";

export default {
  kind: "brain.tool",
  name: "fixture",
  description: "Builder fixture.",
  input: {},
  output: {},
  requiredEnv: [],
  execution: "aex_managed",
  executor: { kind: "aex_managed" },
  contract: { contractDigest: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" },
  execute: async (input) => ({ ...input, escaped: escapeStringRegexp(String(input.value ?? "")) }),
};
