import { environmentProtocol, serveEnvironment, finishExecution } from "@aexhq/brain/runtime";
import { pathToFileURL } from "node:url";

const accepted = () => ({ type: "accepted" });
const failure = (code, message) => ({ type: "failure", code, message, retryable: false });

// Setup records configuration; the first execution allocates the workspace. The caller
// decides when to detach or tear it down; this Environment has no idle policy.
export function lazyEnvironment({ allocate = async () => new Map(), fetch = globalThis.fetch } = {}) {
  const environments = new Map();
  const key = (op) => `${op.session_id}/${op.environment}`;
  const handle = environmentProtocol(async op => {
    const request = op.request;
    let env = environments.get(key(op));
    let receipt;
    if (request?.type === "setup") {
      const options = request.configuration;
      if (!options || typeof options !== "object" || Array.isArray(options)
        || Object.keys(options).some((key) => key !== "label")
        || (options.label !== undefined && typeof options.label !== "string")) {
        receipt = failure("invalid_configuration", "this Environment accepts only an optional label string");
      } else if (env) receipt = failure("already_exists", "Environment already exists");
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
      else if (!request.callback?.methods.includes("finish")) receipt = failure("no_callback", "Tool completion service is required");
      else {
        env.active += 1;
        try {
          env.resource ??= Promise.resolve().then(allocate);
          const resource = await env.resource;
          resource.set(op.sequence, request.input);
          receipt = await finishExecution(op, { type: "result", output: { echo: request.input, entries: resource.size } }, { fetch });
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
    return receipt;
  });
  return { handle };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const env = lazyEnvironment();
  const token = process.env.REFERENCE_ENV_TOKEN;
  await serveEnvironment(env.handle, { token, port: Number(process.env.PORT ?? 8090) });
}
