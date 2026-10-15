import { pathToFileURL } from "node:url";

const chunks = [];
let length = 0;
for await (const chunk of process.stdin) {
  length += chunk.length;
  if (length > 262144) process.exit(65);
  chunks.push(chunk);
}
const { execution, entrypoint } = JSON.parse(Buffer.concat(chunks));
console.log = console.info = console.debug = (...values) => console.error(...values);
let receipt;
try {
  const module = await import(pathToFileURL(`/work/${entrypoint}`).href);
  const action = module.actions?.[execution.operation.action] ?? module.default;
  if (typeof action !== "function") throw new Error("action_not_exported");
  const result = await action(execution.arguments, Object.freeze({
    operationId: execution.operation.id, sessionId: execution.operation.session_id,
    action: execution.operation.action, deadline: execution.deadline_ms,
    signal: AbortSignal.timeout(Math.max(1, execution.deadline_ms - Date.now())),
  }));
  try {
    const serialized = JSON.stringify(result ?? null);
    receipt = Buffer.byteLength(serialized) <= execution.max_output_bytes
      ? { outcome: "completed", result: JSON.parse(serialized) }
      : { outcome: "completed", error: "managed result exceeded its admitted output bound" };
  } catch {
    receipt = { outcome: "completed", error: "managed result was not JSON serializable" };
  }
} catch {
  receipt = { outcome: "failed", error: "managed action failed; secret-bearing exception details are not exported" };
}
process.stdout.write(JSON.stringify({ protocol: "brain-managed-result/1", ...receipt }) + "\n");
