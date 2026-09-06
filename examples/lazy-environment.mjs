import { createServer } from "node:http";
import { pathToFileURL } from "node:url";

const accepted = () => ({ type: "accepted" });
const failure = (code, message) => ({ type: "failure", code, message, retryable: false });
const identifier = (value) => typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(value);

// Setup records needs; the first execution allocates the workspace. The caller
// decides when to detach or tear it down; this Environment has no idle policy.
export function lazyEnvironment({ allocate = async () => new Map() } = {}) {
  const environments = new Map();
  const key = (op) => `${op.session_id}/${op.environment}`;
  async function handle(command) {
    const op = command?.operation;
    if (command?.contract !== "environment/v1" || !op || !Number.isSafeInteger(op.sequence) || op.sequence < 1
      || !identifier(op.session_id) || !identifier(op.environment)) {
      throw new TypeError("invalid Environment command");
    }
    const request = op.request;
    let env = environments.get(key(op));
    let receipt;
    if (request?.type === "setup") {
      const unmet = (request.needs ?? []).find((need) => !need.startsWith("file:"));
      if (unmet !== undefined) receipt = failure("unmet_need", `this Environment offers a workspace only; it cannot honour ${unmet}`);
      else if (env) receipt = failure("already_exists", "Environment already exists");
      else {
        environments.set(key(op), { active: 0 });
        receipt = accepted();
      }
    } else if (request?.type === "teardown") {
      environments.delete(key(op));
      receipt = accepted();
    } else if (!env) {
      receipt = failure("unavailable", "Environment is absent; metadata does not restore its resource");
    } else if (request?.type === "detach") {
      receipt = accepted();
    } else if (request?.type === "execute") {
      if (request.implementation?.type !== "reference_echo") receipt = failure("unsupported", "This example runs only reference_echo Tools");
      else {
        env.active += 1;
        try {
          env.resource ??= Promise.resolve().then(allocate);
          const resource = await env.resource;
          resource.set(op.sequence, request.input);
          receipt = { type: "result", output: { echo: request.input, entries: resource.size } };
        } catch (error) {
          receipt = failure("allocation_failed", String(error.message ?? error));
        } finally {
          env.active -= 1;
        }
      }
    } else if (request?.type === "call" && request.name === "restart") {
      if (env.active) receipt = failure("busy", "Resource has active calls");
      else { env.resource = undefined; receipt = { type: "result", output: { restored: false } }; }
    } else {
      receipt = failure("unsupported", "Operation is unsupported; no effect was retried");
    }
    return { contract: "environment/v1", sequence: op.sequence, receipt };
  }
  return { handle };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const env = lazyEnvironment();
  const token = process.env.REFERENCE_ENV_TOKEN;
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
