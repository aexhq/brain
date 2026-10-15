import { Fault, decode, encode, initialize, need } from "./domain.mjs";
import { controlHandlers } from "./control.mjs";
import { effectHandlers } from "./effects.mjs";
import { resourceHandlers } from "./resources.mjs";
import { timerHandlers } from "./timers.mjs";
import { pageHandlers } from "./pages.mjs";
import { budgetHandlers } from "./budgets.mjs";
import { lineageHandlers } from "./lineage.mjs";
import { childHandlers } from "./children.mjs";
import { transferHandlers } from "./transfers.mjs";

const handlers = { ...controlHandlers, ...effectHandlers, ...resourceHandlers, ...timerHandlers, ...pageHandlers, ...budgetHandlers, ...lineageHandlers,
  ...childHandlers, ...transferHandlers,
  Initialize: async db => { await initialize(db); } };
const counts = {
  Initialize: 0, Clock: 0, RegisterNode: 2, ConfigureNode: 4, RenewNode: 2, RetireNode: 1,
  Create: 5, Delegate: 7, Session: 2, List: 3, ClaimOwner: 2, Accept: 3, Command: 2, Pending: 2, FinishStep: 2,
  NextInput: 2, ApplyInput: 2, Allocate: 2, Written: 5, SealUnused: 3, Allocations: 3, LocateRecord: 2,
  Runs: 3, RunnableRuns: 2, ChildState: 1, Children: 3, SaveDecision: 5, CompleteRun: 5, AdvanceRun: 7, Prepare: 2, Operation: 2, Operations: 3, PendingResults: 2, ClaimDispatch: 5,
  SaveHarnessDecision: 6, BeginDecision: 7,
  Nodes: 2, BeginTransfer: 2, Transfer: 2, PendingTransfers: 3, SealTransfer: 4, TransferRuns: 4,
  VerifyTransferTail: 4, VerifyTransferRuns: 5, CancelTransfer: 2, FailTransfer: 4,
  RecordReceipt: 5, BindResult: 5, ExpireOperations: 1, PropagateControls: 1, Interact: 2, Interactions: 3, Interaction: 2, Answer: 3,
  Attach: 2, Connections: 3, ActiveConnection: 2, RevokeConnection: 2, Documents: 4, PutDocument: 4, DeleteDocument: 3,
  PutFile: 2, Files: 3, File: 2, DeleteFile: 2, Schedule: 2, Schedules: 3, CancelSchedule: 2, FireTimers: 1,
  Grant: 2, Participant: 1, RevokeParticipant: 2, Handoff: 2, Delete: 1, PublishCoverage: 2,
  Coverage: 3, Report: 0, Budget: 1, PutPackage: 2, Package: 2,
};
const status = { invalid: 400, unauthorized: 401, forbidden: 403, not_found: 404, conflict: 409, overloaded: 429,
  unsupported: 422, unavailable: 503, uncertain: 503, corrupt: 500, internal: 500 };
let active = 0;
let leaseRequests = 0;
const leaseOperations = new Set(["Clock", "RegisterNode", "RenewNode", "RetireNode"]);
const bytes = new TextEncoder();

async function body(request, maximum) {
  const reader = request.body?.getReader();
  need(reader, "invalid", "control body is required");
  const chunks = [];
  let size = 0;
  try {
    for (;;) {
      const value = await reader.read();
      if (value.done) break;
      size += value.value.byteLength;
      need(size <= maximum, "invalid", "control request exceeds its byte bound");
      chunks.push(value.value);
    }
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
  const result = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) { result.set(chunk, offset); offset += chunk.length; }
  return new TextDecoder("utf-8", { fatal: true }).decode(result);
}
async function authenticate(request, env, text) {
  need(/^[a-f0-9]{64}$/iu.test(env.BRAIN_D1_KEY ?? ""), "unavailable", "D1 service signing authority is not configured");
  const stamp = request.headers.get("X-Brain-Request-Time");
  need(/^[0-9]{13}$/u.test(stamp ?? "") && Math.abs(Date.now() - Number(stamp)) <= 60000, "unauthorized", "a current control signature is required");
  const signature = request.headers.get("X-Brain-Control-Signature");
  need(/^[a-f0-9]{64}$/u.test(signature ?? ""), "unauthorized", "a control signature is required");
  const key = await crypto.subtle.importKey("raw", Uint8Array.from(env.BRAIN_D1_KEY.match(/../gu), value => Number.parseInt(value, 16)),
    { name: "HMAC", hash: "SHA-256" }, false, ["verify"]);
  const valid = await crypto.subtle.verify("HMAC", key, Uint8Array.from(signature.match(/../gu), value => Number.parseInt(value, 16)),
    bytes.encode("brain-d1/1\0" + new URL(request.url).pathname + "\0" + stamp + "\0" + text));
  need(valid, "unauthorized", "control signature is invalid");
}
function depth(value, level = 0) {
  need(level <= 64, "invalid", "control JSON nesting exceeds its bound");
  if (value !== null && typeof value === "object") for (const item of Object.values(value)) depth(item, level + 1);
}
function failure(error) {
  if (error instanceof Fault) return error;
  const message = String(error?.cause?.message ?? error?.message ?? "");
  if (/CHECK constraint failed|UNIQUE constraint failed|FOREIGN KEY constraint failed/iu.test(message)) {
    return new Fault("conflict", "D1 atomic guard rolled back the domain operation");
  }
  return new Fault("uncertain", "D1 control result is uncertain; no dispatch permission was returned");
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method === "GET" && url.pathname === "/health/live") return new Response(null, { status: 204 });
    const headers = { "Content-Type": "application/json", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff" };
    let admitted = false;
    let lease = false;
    try {
      const operation = url.pathname.slice("/control/v1/".length);
      need(request.method === "POST" && url.pathname.startsWith("/control/v1/") && Object.hasOwn(counts, operation) && !url.search,
        "not_found", "unknown control operation");
      lease = leaseOperations.has(operation);
      need(lease ? leaseRequests < 2 : active < 4, "overloaded", "D1 request decoding capacity is occupied");
      if (lease) leaseRequests++;
      else active++;
      admitted = true;
      const text = await body(request, lease ? 4096 : 2 * 1024 * 1024);
      await authenticate(request, env, text);
      const envelope = JSON.parse(text);
      need(Object.keys(envelope).every(key => key === "arguments" || key === "authority") &&
        Array.isArray(envelope.arguments) && envelope.arguments.length === counts[operation] &&
        envelope.arguments.every(value => typeof value === "string"), "invalid", "control arguments do not match the finite domain operation");
      const authority = envelope.authority;
      if (authority !== undefined) {
        need(authority && Object.keys(authority).length === 3 &&
          ["Participant", "Session", "Tenant"].every(key => typeof authority[key] === "string" &&
            /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(authority[key])),
        "invalid", "control participant authority is invalid");
      }
      const values = envelope.arguments.map(decode);
      depth(values);
      const db = { authority, prepare: sql => env.DB.prepare(sql), batch: statements => env.DB.batch(statements),
        withSession: mode => env.DB.withSession(mode) };
      const value = await handlers[operation](db, values, envelope.arguments);
      const response = encode({ value: value ?? null });
      need(bytes.encode(response).byteLength <= 4 * 1024 * 1024, "overloaded", "control response exceeds its page bound");
      return new Response(response, { status: 200, headers });
    } catch (error) {
      const errorResponse = failure(error);
      return new Response(encode({ code: errorResponse.code, message: errorResponse.message, retryable: errorResponse.code === "overloaded" }),
        { status: status[errorResponse.code] ?? 500, headers });
    } finally {
      if (admitted) {
        if (lease) leaseRequests--;
        else active--;
      }
    }
  },
};
