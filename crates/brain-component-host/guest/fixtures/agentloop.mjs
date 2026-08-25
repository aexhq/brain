import { call } from "aex:agentloop/context@1.0.0";

let activations = 0;
let memory = [];
let operations = 0;

export function activate(request) {
  const config = JSON.parse(request.configJson);
  if (config.track === true) {
    activations += 1;
    return { payloadJson: JSON.stringify({ activations }) };
  }
  if (config.fixture === "sequential") {
    const payload = JSON.parse(request.payloadJson);
    if (request.kind === "session_start") {
      memory = [];
      return completed(payload.activation_id);
    }
    memory.push({ role: "user", content: payload.message.content });
    for (;;) {
      const round = op(payload.activation_id, { op: "model_stream", request: { messages: memory } }).message;
      memory.push({ role: "assistant", content: round.content });
      const calls = round.content.filter((block) => block.type === "tool_call");
      if (calls.length === 0) break;
      const results = op(payload.activation_id, {
        op: "tools_dispatch",
        calls: calls.map((item) => ({
          tool_call_id: item.tool_call_id,
          name: item.name,
          input: item.input,
        })),
      }).results;
      for (const result of results) memory.push({ role: "tool_result", ...result });
    }
    op(payload.activation_id, { op: "turn_finish" });
    return completed(payload.activation_id);
  }
  return { payloadJson: request.payloadJson };
}

function op(activationId, value) {
  const opId = `fixture-${++operations}`;
  const response = JSON.parse(call(opId, JSON.stringify({ op_id: opId, activation_id: activationId, op: value })));
  if (response.error !== undefined) throw new Error(response.error.message);
  return response.result;
}

function completed(activationId) {
  return { payloadJson: JSON.stringify({ activation_id: activationId, outcome: "completed" }) };
}
