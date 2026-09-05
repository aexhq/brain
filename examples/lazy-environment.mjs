import { createServer } from "node:http";
import { pathToFileURL } from "node:url";

const accepted = () => ({ type: "accepted", resources: {} });
const failure = (code, message) => ({ type: "failure", code, message, retryable: false });
const identifier = (value) => typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(value);

// The example resource is an ephemeral key/value workspace. Allocation may be asynchronous.
export function lazyEnvironment({ allocate = async () => new Map(), now = () => performance.now() } = {}) {
  const environments = new Map();
  function expire() {
    for (const env of environments.values()) {
      if (env.resource && env.active === 0 && now() - env.used >= env.ttl) {
        env.resource = undefined;
        env.expired = true;
      }
    }
  }
  async function handle(command) {
    const op = command?.operation;
    if (command?.contract !== "environment/v1" || !op || !Number.isSafeInteger(op.sequence) || op.sequence < 1
      || !identifier(op.session_id) || !identifier(op.environment_id) || command.binding?.environment_id !== op.environment_id) {
      throw new TypeError("invalid Environment command");
    }
    expire();
    const request = op.request;
    let env = environments.get(op.environment_id);
    let receipt;
    if (request?.type === "setup") {
      const ttl = request.configuration?.idle_ms ?? 30_000;
      if (!Number.isSafeInteger(ttl) || ttl <= 0) receipt = failure("invalid_ttl", "idle_ms must be a positive integer");
      else if (env) receipt = failure("already_exists", "Environment already exists");
      else {
        environments.set(op.environment_id, { session: op.session_id, ttl, used: now(), active: 0, tools: new Map(), expired: false });
        receipt = accepted();
      }
    } else if (!env || env.session !== op.session_id) {
      receipt = failure("unavailable", "Environment is absent; metadata does not restore its resource");
    } else if (request?.type === "attach") {
      if (!identifier(op.attachment_id) || !Array.isArray(request.provisions)
        || request.provisions.some(({ manifest }) => !identifier(manifest?.name) || manifest.implementation?.type !== "reference_echo")) {
        receipt = failure("unsupported", "This example accepts only reference_echo Tools");
      } else {
        env.attachment = op.attachment_id;
        env.tools = new Map(request.provisions.map(({ manifest }) => [manifest.name, manifest]));
        receipt = accepted();
      }
    } else if (request?.type === "teardown") {
      environments.delete(op.environment_id);
      receipt = accepted();
    } else if (op.attachment_id !== env.attachment || !env.attachment) {
      receipt = failure("not_attached", "Operation does not name the active attachment");
    } else if (request?.type === "detach") {
      env.attachment = undefined;
      receipt = accepted();
    } else if (request?.type === "invoke") {
      if (!env.tools.has(request.tool)) receipt = failure("unbound_tool", "Tool is not attached");
      else if (env.expired) receipt = failure("expired", "Resource expired; restart is explicit and loses prior contents");
      else {
        env.active += 1;
        try {
          env.resource ??= Promise.resolve().then(allocate);
          const resource = await env.resource;
          resource.set(request.call_id, request.input);
          receipt = { type: "outcome", outcome: { status: "ok", value: { echo: request.input, entries: resource.size } } };
        } catch (error) {
          receipt = failure("allocation_failed", String(error.message ?? error));
        } finally {
          env.active -= 1;
          env.used = now();
        }
      }
    } else if (request?.type === "call" && request.name === "restart") {
      if (env.active) receipt = failure("busy", "Resource has active calls");
      else { env.resource = undefined; env.expired = false; receipt = { type: "result", output: { restored: false } }; }
    } else {
      receipt = failure("unsupported", "Operation is unsupported; no effect was retried");
    }
    return { contract: "environment/v1", sequence: op.sequence, receipt };
  }
  return { handle, expire };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const env = lazyEnvironment();
  const token = process.env.REFERENCE_ENV_TOKEN;
  setInterval(env.expire, 1000).unref();
  createServer(async (req, res) => {
    if (token && req.headers.authorization !== `Bearer ${token}`) { res.writeHead(401).end(); return; }
    if (req.method !== "POST" || req.url !== "/v1/operations") { res.writeHead(404).end(); return; }
    try {
      const chunks = [];
      let bytes = 0;
      for await (const chunk of req) {
        bytes += chunk.length;
        if (bytes > 2 * 1024 * 1024) throw new TypeError("command exceeds 2 MiB");
        chunks.push(chunk);
      }
      const response = await env.handle(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(response));
    } catch (error) { res.writeHead(400).end(String(error.message ?? error)); }
  }).listen(Number(process.env.PORT ?? 8090), "127.0.0.1");
}
