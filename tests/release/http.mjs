import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";


const baseUrl = process.env.BRAIN_BASE_URL;
const token = process.env.BRAIN_API_TOKEN;
assert.ok(baseUrl);
assert.ok(token);

let operation = 0;
const request = async (method, path, body, contentType = "application/json") => {
  operation += 1;
  const headers = {
    accept: "application/json",
    authorization: `Bearer ${token}`,
    "idempotency-key": `release-http-${operation}`,
  };
  if (body !== undefined) headers["content-type"] = contentType;
  const response = await fetch(`${baseUrl}${path}`, {
    method,
    headers,
    body: body === undefined
      ? undefined
      : contentType === "application/json"
        ? JSON.stringify(body)
        : body,
  });
  const result = response.status === 204 ? undefined : await response.json();
  assert.ok(response.ok, `${method} ${path} returned ${response.status}: ${JSON.stringify(result)}`);
  return result;
};

const admission = await request(
  "POST",
  "/v1/agentloops",
  await readFile(new URL("./built/diagnostic.brain.json", import.meta.url)),
  "application/octet-stream",
);
assert.equal(admission.status, "admitted");
const session = await request("POST", "/v1/sessions", {
  agentloop_identity: admission.identity,
  brain_configuration: {},
  model: {
    provider: "vercel-ai-gateway",
    name: "openai/gpt-5-mini",
    api_key: "release-smoke-key",
  },
  presentation: { system: "", tools: [] },
  environments: [],
  tool_bindings: [],
});
assert.equal(session.status, "idle");
assert.equal((await request("GET", `/v1/sessions/${session.session_id}`)).session_id, session.session_id);
assert.ok((await request("GET", "/v1/sessions")).sessions.some(({ session_id }) => session_id === session.session_id));
assert.equal((await request("POST", `/v1/sessions/${session.session_id}/end`)).status, "ended");
await request("DELETE", `/v1/sessions/${session.session_id}`);
assert.deepEqual((await request("GET", "/v1/sessions")).sessions, []);
