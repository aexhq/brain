import { createServer } from "node:http";
import { pathToFileURL } from "node:url";

const accepted = () => ({ type: "accepted" });
const failure = (code, message) => ({ type: "failure", code, message, retryable: false });
const identifier = (value) => typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(value);

// A standalone Environment that runs an Agentloop outside Brain's process. With every
// turn Brain sends the loop's id and needs, the turn input, and a callback: the address
// of Brain's turn services for that turn and the token that opens them. This loop's
// policy is the smallest possible one: hand the model the transcript plus the user's
// message, append its answer, and return. Anything the loop needs from Brain during the
// turn goes through the callback; every call is journaled by Brain like an in-process
// loop's. `fetch` is injectable so the loop can be driven without a network.
export function loopEnvironment({ fetch = globalThis.fetch } = {}) {
  const sessions = new Set();
  const call = async (callback, path, body) => {
    const response = await fetch(`${callback.url}/${path}`, {
      method: "POST",
      headers: { authorization: `Bearer ${callback.token}`, "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!response.ok) throw new Error(`${path} answered ${response.status}: ${await response.text()}`);
    return response.json();
  };
  async function turn(callback, input) {
    const transcript = [...input.transcript, { role: "user", content: [{ type: "text", text: input.input.message }] }];
    await call(callback, "emit", { event_type: "remote_note", data: { turns: (input.slots.turns ?? 0) + 1 } });
    const result = await call(callback, "model", { messages: transcript });
    transcript.push(result.message);
    return { transcript, slots: { turns: (input.slots.turns ?? 0) + 1 }, result: { stop_reason: result.stop_reason } };
  }
  async function handle(command) {
    const op = command?.operation;
    if (command?.contract !== "environment/v1" || !op || !Number.isSafeInteger(op.sequence) || op.sequence < 1
      || !identifier(op.session_id) || !identifier(op.environment)) {
      throw new TypeError("invalid Environment command");
    }
    const request = op.request;
    const key = `${op.session_id}/${op.environment}`;
    let receipt;
    if (request?.type === "setup") {
      const unmet = (request.needs ?? []).find((need) => !need.startsWith("https://"));
      if (unmet !== undefined) receipt = failure("unmet_need", `this Environment reaches the network only; it cannot honour ${unmet}`);
      else { sessions.add(key); receipt = accepted(); }
    } else if (request?.type === "teardown") {
      sessions.delete(key);
      receipt = accepted();
    } else if (!sessions.has(key)) {
      receipt = failure("unavailable", "Environment is absent");
    } else if (request?.type === "turn") {
      if (!request.callback) receipt = failure("no_callback", "a turn outside Brain needs its callback");
      else {
        try {
          receipt = { type: "turned", output: await turn(request.callback, request.input) };
        } catch (error) {
          receipt = failure("agentloop_failed", String(error.message ?? error));
        }
      }
    } else if (request?.type === "detach" || request?.type === "cancel") {
      receipt = accepted();
    } else {
      receipt = failure("unsupported", "this Environment runs an Agentloop and nothing else");
    }
    return { contract: "environment/v1", sequence: op.sequence, receipt };
  }
  return { handle };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const env = loopEnvironment();
  const token = process.env.REFERENCE_ENV_TOKEN;
  createServer(async (req, res) => {
    if (token && req.headers.authorization !== `Bearer ${token}`) { res.writeHead(401).end(); return; }
    if (req.method !== "POST" || req.url !== "/v1/operations") { res.writeHead(404).end(); return; }
    try {
      const chunks = [];
      let bytes = 0;
      for await (const chunk of req) {
        bytes += chunk.length;
        if (bytes > 32 * 1024 * 1024) throw new TypeError("command exceeds 32 MiB");
        chunks.push(chunk);
      }
      const response = await env.handle(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(response));
    } catch (error) { res.writeHead(400).end(String(error.message ?? error)); }
  }).listen(Number(process.env.PORT ?? 8091), "127.0.0.1");
}
