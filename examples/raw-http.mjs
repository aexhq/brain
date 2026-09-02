import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";


const apiKey = process.env.VERCEL_AI_GATEWAY_API_KEY;
if (!apiKey) throw new Error("VERCEL_AI_GATEWAY_API_KEY is required");

const baseUrl = process.env.BRAIN_BASE_URL ?? "http://127.0.0.1:8080";
const token = process.env.BRAIN_API_TOKEN;

async function request(method, path, body, contentType = "application/json") {
  const headers = { accept: "application/json" };
  if (method !== "GET") headers["idempotency-key"] = randomUUID();
  if (token) headers.authorization = `Bearer ${token}`;
  if (body !== undefined) headers["content-type"] = contentType;

  const response = await fetch(`${baseUrl}${path}`, {
    method,
    headers,
    body: body === undefined
      ? undefined
      : body instanceof Uint8Array
        ? body
        : JSON.stringify(body),
    signal: AbortSignal.timeout(30_000),
  });
  const text = await response.text();
  if (!response.ok) throw new Error(`${method} ${path} returned ${response.status}: ${text}`);
  return text ? JSON.parse(text) : undefined;
}

const packageBytes = new Uint8Array(await readFile(new URL("./dist/example.brain.json", import.meta.url)));

const admission = await request("POST", "/v1/agentloops", packageBytes, "application/octet-stream");
const session = await request("POST", "/v1/sessions", {
  agentloop: { identity: admission.identity, configuration: { system: "Answer briefly." } },
  model: {
    provider: "vercel-ai-gateway",
    name: process.env.BRAIN_MODEL ?? "openai/gpt-5-mini",
    api_key: apiKey,
  },
  tools: [],
  environments: [],
});

try {
  await request("POST", `/v1/sessions/${session.session_id}/messages`, {
    input: { message: "Reply with HTTP_OK." },
  });
  console.log(await request("GET", `/v1/sessions/${session.session_id}/events?after=0`));
} finally {
  await request("POST", `/v1/sessions/${session.session_id}/end`);
  await request("DELETE", `/v1/sessions/${session.session_id}`);
}
