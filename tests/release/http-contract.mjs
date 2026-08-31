import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const baseUrl = process.env.BRAIN_BASE_URL;
const token = process.env.BRAIN_API_TOKEN;
assert.ok(baseUrl);
assert.ok(token);

const call = async (method, path, { body, contentType = "application/json", key, authorized = true, accept = "application/json" } = {}) => {
  const headers = { accept };
  if (authorized) headers.authorization = `Bearer ${token}`;
  if (key !== undefined) headers["idempotency-key"] = key;
  if (body !== undefined) headers["content-type"] = contentType;
  const response = await fetch(`${baseUrl}${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : contentType === "application/json" ? JSON.stringify(body) : body,
  });
  const text = response.status === 204 ? "" : await response.text();
  let result = text;
  if (text !== "" && response.headers.get("content-type")?.startsWith("application/json")) result = JSON.parse(text);
  return { response, result };
};

/// Reads an open SSE response only until `pattern` appears, then cancels it. Bounded, so
/// a stream that never carries what was asked for fails the test instead of hanging it.
const streamCarries = async (response, pattern, timeoutMs = 30_000) => {
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const deadline = setTimeout(() => reader.cancel(), timeoutMs);
  let seen = "";
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return pattern.test(seen);
      seen += decoder.decode(value, { stream: true });
      if (pattern.test(seen)) return true;
    }
  } finally {
    clearTimeout(deadline);
    await reader.cancel().catch(() => {});
  }
};

for (const path of ["/health/live", "/health/ready"]) {
  const { response } = await call("GET", path, { authorized: false });
  assert.equal(response.status, 204);
}

const unauthorized = await call("GET", "/v1/sessions", { authorized: false });
assert.equal(unauthorized.response.status, 401);
assert.equal(unauthorized.result.code, "unauthorized");

const missingKey = await call("POST", "/v1/agentloops", {
  body: new Uint8Array([1]),
  contentType: "application/octet-stream",
});
assert.equal(missingKey.response.status, 400);
assert.equal(missingKey.result.code, "invalid_request");

const packageBytes = await readFile(new URL("./built/diagnostic.brain.json", import.meta.url));
const admitted = await call("POST", "/v1/agentloops", {
  body: packageBytes,
  contentType: "application/octet-stream",
  key: "http-contract-admit",
});
assert.equal(admitted.response.status, 200);
assert.equal(admitted.result.status, "admitted");
const admission = await call("GET", `/v1/agentloops/${admitted.result.identity}`);
assert.deepEqual(admission.result, admitted.result);

const createBody = {
  agentloop: { identity: admitted.result.identity, configuration: {} },
  model: { provider: "vercel-ai-gateway", name: "openai/gpt-5-mini", api_key: "release-smoke-key" },
  system: "",
  tools: [],
  environments: [],
};
const created = await call("POST", "/v1/sessions", { body: createBody, key: "http-contract-create" });
assert.equal(created.response.status, 200);
assert.equal(created.result.status, "idle");

const replayedCreate = await call("POST", "/v1/sessions", { body: createBody, key: "http-contract-create" });
assert.deepEqual(replayedCreate.result, created.result);
const conflictingCreate = await call("POST", "/v1/sessions", {
  body: { ...createBody, system: "changed" },
  key: "http-contract-create",
});
assert.equal(conflictingCreate.response.status, 409);
assert.equal(conflictingCreate.result.code, "conflict");

const unknownField = await call("POST", "/v1/sessions", {
  body: { ...createBody, unknown: true },
  key: "http-contract-unknown",
});
assert.equal(unknownField.response.status, 422);

const sessionId = created.result.session_id;
const message = await call("POST", `/v1/sessions/${sessionId}/messages`, {
  body: { content: "finish locally" },
  key: "http-contract-message",
});
assert.equal(message.response.status, 200);
assert.equal(message.result.status, "idle");
const replayedMessage = await call("POST", `/v1/sessions/${sessionId}/messages`, {
  body: { content: "finish locally" },
  key: "http-contract-message",
});
assert.deepEqual(replayedMessage.result, message.result);

const page = await call("GET", `/v1/sessions/${sessionId}/events?after=0`);
assert.equal(page.response.status, 200);
assert.equal(page.result.next_cursor, message.result.last_sequence);
assert.equal(page.result.events.at(-1).event_type, "turn_finished");
// Read until the event we came for, not to the end of the body: the stream stays open
// carrying records as they are appended, which is what makes a first-token measurement
// possible. Reading it to EOF would wait for a close that only a session ending brings.
const sse = await fetch(`${baseUrl}/v1/sessions/${sessionId}/events?after=0`, {
  headers: { accept: "text/event-stream", authorization: `Bearer ${token}` },
});
assert.equal(sse.status, 200);
assert.ok(sse.headers.get("content-type")?.startsWith("text/event-stream"));
assert.ok(await streamCarries(sse, /event: turn_finished/u), "the stream never carried the finished turn");

const cancelled = await call("POST", `/v1/sessions/${sessionId}/cancel`, { key: "http-contract-cancel" });
assert.equal(cancelled.response.status, 204);
const ended = await call("POST", `/v1/sessions/${sessionId}/end`, { key: "http-contract-end" });
assert.equal(ended.result.status, "ended");
assert.ok((await call("GET", "/v1/sessions")).result.sessions.some((session) => session.session_id === sessionId));
assert.equal((await call("GET", `/v1/sessions/${sessionId}`)).result.session_id, sessionId);
assert.equal((await call("DELETE", `/v1/sessions/${sessionId}`, { key: "http-contract-delete" })).response.status, 204);
assert.equal((await call("GET", `/v1/sessions/${sessionId}`)).response.status, 404);
