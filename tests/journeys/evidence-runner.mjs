// Protocol conformance fixture. Native adapter qualification uses real runners.
import { randomBytes } from "node:crypto";
import readline from "node:readline";
const sessions = new Map(), pending = new Map();
const id = () => randomBytes(16).toString("hex");
const send = frame => process.stdout.write(JSON.stringify(frame) + "\n");
const unknown = () => ({ availability: "unknown", semantics: "snapshot", input_cache: "unknown", output_reasoning: "unknown" });
async function observe(state, data) {
  const token = id();
  await new Promise((resolve, reject) => {
    pending.set(token, { resolve, reject });
    send({ attachment: state.attachment, turn: state.turn, id: token, method: "model.request", params: data });
  });
}
async function prompt(state, text) {
  state.abort = new AbortController();
  for (const purpose of text === "all paths" ? ["primary", "compaction", "other"] : ["primary"]) {
    const logical = { request_id: id(), purpose, boundary: "request", state: "started", harness: "", provider: "", model: "", attempts: "observed", usage: unknown() };
    const attempt = { ...logical, attempt_id: id() };
    await observe(state, logical); await observe(state, logical);
    await observe(state, attempt);
    try {
      const response = await fetch(state.options.provider.base_url + "/chat/completions", {
        method: "POST", headers: { Authorization: `Bearer ${state.options.provider.api_key}`, "Content-Type": "application/json" },
        body: JSON.stringify({ model: "gpt-4.1-mini", messages: [{ role: "user", content: logical.request_id }], tools: [] }), signal: state.abort.signal,
      });
      await response.text();
      if (text === "omit outcome") return {};
      const usage = text === "unknown usage" ? unknown() : { ...unknown(), availability: "partial", input_tokens: 10, input_cache: "excluded", output_reasoning: "included" };
      attempt.state = logical.state = response.ok ? "completed" : "failed";
      attempt.provider_request_id = response.headers.get("x-request-id") ?? "";
      attempt.usage = usage;
      await observe(state, attempt);
      if (text !== "unknown usage") {
        attempt.usage = { ...usage, availability: "reported", output_tokens: 4, cache_read_tokens: 2, reasoning_tokens: 1 };
        await observe(state, attempt); await observe(state, attempt);
      }
      await observe(state, logical);
    } catch (error) {
      if (!state.abort.signal.aborted) throw error;
      attempt.state = logical.state = "cancelled";
      await observe(state, attempt); await observe(state, logical);
    }
  }
  send({ attachment: state.attachment, turn: state.turn, event: "assistant.message", data: { text: "observed" } });
  return {};
}
async function receive(frame) {
  if (!frame.method) {
    const waiter = pending.get(frame.id); pending.delete(frame.id);
    if (!waiter) throw new Error("unexpected acknowledgment");
    if (frame.error) waiter.reject(new Error("observation rejected")); else waiter.resolve();
    return;
  }
  let state = sessions.get(frame.attachment);
  if (frame.method === "open") { state = { attachment: frame.attachment, options: frame.params }; sessions.set(frame.attachment, state); }
  try {
    let result = {};
    if (frame.method === "prompt") { state.turn = frame.turn; result = await prompt(state, frame.params.text); }
    else if (frame.method === "interrupt") state.abort?.abort();
    else if (frame.method === "checkpoint") result = [];
    else if (frame.method === "detach") sessions.delete(frame.attachment);
    send({ attachment: frame.attachment, turn: frame.turn, id: frame.id, result });
  } catch {
    send({ attachment: frame.attachment, turn: frame.turn, id: frame.id, error: { code: "brain_error", message: "fixture execution failed" } });
  }
}
readline.createInterface({ input: process.stdin }).on("line", line => { void receive(JSON.parse(line)); });
process.stdin.on("end", () => process.exit(0));
