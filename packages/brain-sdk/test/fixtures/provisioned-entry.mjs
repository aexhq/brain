// Build fixture for the tool artifact golden tests: one program of each kind.
import { tool } from "@aexhq/brain";
import { z } from "zod";

export const echo = tool({
  description: "Echo the text beside the injected base URL.",
  input: z.object({ text: z.string() }),
  output: z.object({ echoed: z.string() }),
  needs: ["process"],
  bindings: { API_BASE: z.string() },
}, (author) => {
  author.run(async (input, context) => ({ echoed: `${input.text} # ${context.bindings.API_BASE}` }));
});

export const bash = tool.shell({
  description: "Run a shell command in the session workspace.",
  input: z.object({ command: z.string() }),
  output: z.object({ exit_code: z.number(), stdout: z.string(), stderr: z.string() }),
  needs: ["process"],
  script: "$command",
});

export const ping = tool.http({
  description: "Ping the environment's own service.",
  input: z.object({ payload: z.string() }),
  request: { method: "POST", url: "https://service.internal/ping", headers: { "x-source": "brain" } },
});
