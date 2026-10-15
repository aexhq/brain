import readline from "node:readline";

// Independent protocol fixture: no network, provider SDK or application effects.
const lines = readline.createInterface({ input: process.stdin });
for await (const line of lines) {
  const request = JSON.parse(line);
  if (request.children?.length) {
    process.stdout.write(`${JSON.stringify({ kind: "complete", result: { children: request.children }, checkpoint: {} })}\n`);
  } else if (/^fixture:(fork|delegate):/u.test(request.input.text ?? "")) {
    const [, kind, names] = request.input.text.split(":");
    const delegations = names.split(",").map((message, index) => ({
      request_id: "fixture-child-" + index, message, context: [],
      config: { ...request.config, limits: { ...request.config.limits, calls: 1, concurrent: 1 } },
    }));
    process.stdout.write(`${JSON.stringify({ kind, ...(kind === "fork" ? { delegations } : { delegation: delegations[0] }), checkpoint: {} })}\n`);
  } else if ((request.input.text ?? "").startsWith("fixture:action:")) {
    process.stdout.write(`${JSON.stringify(request.outcome
      ? { kind: "complete", result: request.outcome.result, checkpoint: {} }
      : { kind: "call", effect: "action", action: request.input.text.slice("fixture:action:".length), arguments: {}, checkpoint: {} })}\n`);
  } else if (request.outcome) {
    process.stdout.write(`${JSON.stringify({
      kind: "complete", result: request.outcome.result.message, checkpoint: {},
    })}\n`);
  } else {
    const messages = request.context
      .filter(record => record.kind === "run.completed" && record.data.result?.role === "assistant")
      .map(record => record.data.result);
    messages.push({ role: "user", content: request.input.text ?? JSON.stringify(request.input) });
    process.stdout.write(`${JSON.stringify({
      kind: "call", effect: "model", action: "generate",
      arguments: { messages, actions: request.config.actions ?? [] }, checkpoint: { awaiting: "model" },
    })}\n`);
  }
}
