import { createHash } from "node:crypto";
import { readFile, rename, writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const [bundlePath, requestPath, resultPath] = process.argv.slice(2);
if (bundlePath === undefined || requestPath === undefined || resultPath === undefined) {
  process.stderr.write("local Tool runner requires bundle, request, and result paths\n");
  process.exit(70);
}

const abort = new AbortController();
process.on("SIGTERM", () => abort.abort(new Error("Tool call cancelled")));
process.on("SIGINT", () => abort.abort(new Error("Tool call cancelled")));

let request;
const writeResult = async (value) => {
  const encoded = JSON.stringify(value);
  if (!Number.isSafeInteger(request.max_output_bytes) || request.max_output_bytes < 1) {
    throw new TypeError("Tool request has an invalid output ceiling");
  }
  if (Buffer.byteLength(encoded) > request.max_output_bytes) {
    throw new RangeError("Tool result exceeds the sealed output ceiling");
  }
  const temporary = `${resultPath}.tmp`;
  await writeFile(temporary, encoded, { mode: 0o600 });
  await rename(temporary, resultPath);
};

try {
  request = JSON.parse(await readFile(requestPath, "utf8"));
  const bundleBytes = await readFile(bundlePath);
  if (createHash("sha256").update(bundleBytes).digest("hex") !== request.seal.bundle_digest) {
    throw new TypeError("bundle bytes do not match the sealed digest");
  }

  // This is intentionally the first evaluation of customer code. Brain has already recorded
  // the operation intent and the exact staged bytes were verified immediately above.
  const loaded = await import(`${pathToFileURL(bundlePath).href}?operation=${encodeURIComponent(request.operation_id)}`);
  const tool = loaded.default;
  if (tool === null || typeof tool !== "object" || tool.kind !== "brain.tool-runtime") {
    throw new TypeError("bundle default export is not a Brain Tool runtime");
  }
  if (
    tool.name !== request.seal.name
    || (tool.description ?? null) !== (request.seal.description ?? null)
    || tool.contractDigest !== request.seal.contract_digest
  ) {
    throw new TypeError("bundle Tool runtime does not match the sealed contract");
  }
  if (typeof tool.execute !== "function") throw new TypeError("bundle Tool has no executable handler");
  const required = Array.isArray(tool.requiredEnv) ? tool.requiredEnv : [];
  if (JSON.stringify(required) !== JSON.stringify(request.seal.required_env)) {
    throw new TypeError("bundle required environment names do not match the execution seal");
  }
  for (const name of required) {
    if (process.env[name] === undefined) throw new Error(`required environment variable ${name} is unavailable`);
  }

  const input = typeof tool.input?.parseAsync === "function"
    ? await tool.input.parseAsync(request.input)
    : request.input;
  const value = await tool.execute(input, {
    signal: abort.signal,
    operationId: request.operation_id,
    sessionId: request.session_id,
    workspace: request.workspace,
    deadlineMs: request.deadline_ms,
  });
  const output = typeof tool.output?.parseAsync === "function"
    ? await tool.output.parseAsync(value)
    : value;
  JSON.stringify(output);
  await writeResult({ ok: true, output });
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`${message}\n`);
  try {
    await writeResult({ ok: false, error: message });
  } catch (writeError) {
    process.stderr.write(`could not persist Tool result: ${String(writeError)}\n`);
  }
  process.exitCode = 1;
}
