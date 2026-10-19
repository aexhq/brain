import { randomUUID } from "node:crypto";

// A complete Responses SSE lifecycle consumed by the actual pinned adapters.
export function responsesReply(res, answer, usage, model = "gpt-4.1-mini") {
  if (res.modelStreaming === false) {
    const item = typeof answer === "string" ? { id: "msg_" + randomUUID(), type: "message", role: "assistant", status: "completed", content: [{ type: "output_text", text: answer, annotations: [] }] } : { id: "fc_" + randomUUID(), type: "function_call", call_id: "call_" + randomUUID(), name: answer.name, arguments: JSON.stringify(answer.args), status: "completed" };
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ id: "resp_" + randomUUID(), object: "response", created_at: 1, status: "completed", model, output: [item], ...(usage ? { usage } : {}) }));
    return;
  }
  res.writeHead(200, { "Content-Type": "text/event-stream" });
  let sequence = 0;
  const send = (type, data) => res.write(`event: ${type}\ndata: ${JSON.stringify({ type, sequence_number: sequence++, ...data })}\n\n`);
  const response = { id: "resp_" + randomUUID(), object: "response", created_at: 1, status: "in_progress", model, output: [], usage: null };
  send("response.created", { response });
  let item;
  if (typeof answer === "string") {
    item = { id: "msg_" + randomUUID(), type: "message", role: "assistant", status: "in_progress", content: [] };
    send("response.output_item.added", { output_index: 0, item });
    send("response.content_part.added", { output_index: 0, item_id: item.id, content_index: 0, part: { type: "output_text", text: "", annotations: [] } });
    send("response.output_text.delta", { output_index: 0, item_id: item.id, content_index: 0, delta: answer });
    item = { ...item, status: "completed", content: [{ type: "output_text", text: answer, annotations: [] }] };
    send("response.output_text.done", { output_index: 0, item_id: item.id, content_index: 0, text: answer });
    send("response.content_part.done", { output_index: 0, item_id: item.id, content_index: 0, part: item.content[0] });
  } else {
    item = { id: "fc_" + randomUUID(), type: "function_call", call_id: "call_" + randomUUID(), name: answer.name, arguments: "", status: "in_progress" };
    send("response.output_item.added", { output_index: 0, item });
    const args = JSON.stringify(answer.args);
    send("response.function_call_arguments.delta", { output_index: 0, item_id: item.id, delta: args });
    send("response.function_call_arguments.done", { output_index: 0, item_id: item.id, arguments: args });
    item = { ...item, arguments: args, status: "completed" };
  }
  send("response.output_item.done", { output_index: 0, item });
  send("response.completed", { response: { ...response, status: "completed", output: [item], ...(usage ? { usage } : {}) } });
  res.end();
}
