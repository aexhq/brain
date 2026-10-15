import test from "node:test";
import assert from "node:assert/strict";
import { mkdir, rm } from "node:fs/promises";
import { randomUUID, createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { LosslessNumber } from "lossless-json";
import { startD1 } from "../../../tests/fixtures/d1.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const integer = value => new LosslessNumber(String(value));
const hash = value => createHash("sha256").update(value).digest("hex");
const stableID = (...values) => hash(JSON.stringify(values)).slice(0, 40);

async function fixture(t, options = {}) {
  const directory = path.join(root, ".test-output", "d1-" + randomUUID());
  await mkdir(directory, { recursive: true });
  const binding = await startD1(directory);
  t.after(async () => { await binding.dispose(); await rm(directory, { recursive: true, force: true }); });
  async function call(name, args = []) {
    const response = await binding.rpc(name, args);
    assert.equal(response.status, 200, `${name}: ${response.text}`);
    return response.data.value;
  }
  await call("Initialize");
  const node = "binding-owner";
  const custody = hash("retained-custody");
  await call("RegisterNode", [node, 60000]);
  await call("ConfigureNode", [node, "http://retained", custody, ["storage:retained", ...(options.capabilities ?? [])]]);
  const config = { model: { provider: "fixture", name: "fixture" }, harness: { name: "fixture", version: "1" },
    actions: [], connections: [], events: [], grants: [], limits: {
      calls: 100, concurrent: 64, input_bytes: 65536, output_bytes: 65536, retained_bytes: 16777216, pending_inputs: 128,
    }, ...options.config };
  const session = await call("Create", ["tenant", { request_id: "created", config }, "", "", ["storage:retained"]]);
  const id = session.View.id;
  const owner = await call("ClaimOwner", [id, node]);
  async function prepare(name = "once") {
    const command = await call("Accept", [{ Tenant: "tenant", Subject: "operator" }, id,
      { request_id: name, data: { kind: "message", text: name } }]);
    await call("ApplyInput", [id, { Command: command.id, Owner: owner, Sequence: 1, Run: true, Disposition: "", StateKey: "" }]);
    const sequence = await call("Allocate", [id, owner]);
    const digest = hash("intent:" + name);
    await call("Written", [id, owner, sequence, digest, 64]);
    return call("Prepare", [owner, {
      ID: name, Session: id, Run: stableID("run", id, command.id), StepKey: "0", Kind: "model", Action: "generate",
      Arguments: { escaped: "quote\"slash\\", nested: { number: integer("9007199254740993"), empty: null } },
      Recipient: "model", OutputLimit: 8192, Timeout: 30000, Generation: "", Approval: false, RetryOf: "",
      IntentSequence: sequence, IntentDigest: digest,
    }]);
  }
  return { ...binding, call, id, owner, config, custody, prepare };
}

test("actual D1 transfers only a frozen covered checkpoint after exact target proof pages", async t => {
  const f = await fixture(t);
  const operation = await f.prepare("retained-operation");
  await f.call("BeginDecision", [f.id, f.owner, operation.View.run_id, 0, hash("request"), "source-process", true]);
  const target = "transfer-target";
  await f.call("RegisterNode", [target, 60000]);
  await f.call("ConfigureNode", [target, "http://target", f.custody,
    ["storage:retained", "continuation:" + stableID("fixture", "1", "fixture")]]);
  const request = { request_id: "move", owner_epoch: "1", target_node: target };
  const transfer = await f.call("BeginTransfer", [f.id, request]);
  assert.equal((await f.rpc("ClaimDispatch", [f.id, f.owner, operation.View.id, "sender", "claim"])).status, 409);
  assert.equal((await f.rpc("NextInput", [f.id, f.owner])).status, 409);
  const sequence = String(await f.call("Allocate", [f.id, f.owner]));
  const record = { session_id: f.id, sequence, kind: "ownership.checkpoint", time_ms: 1, data: {
    format: "brain-ownership-checkpoint/1", transfer_id: transfer.View.id, source_epoch: "1", target_node: target,
  } };
  assert.equal((await f.rpc("SealTransfer", [f.id, f.owner, transfer.View.id, record])).status, 409);
  await f.call("Written", [f.id, f.owner, integer(sequence), hash(JSON.stringify(record)), Buffer.byteLength(JSON.stringify(record))]);
  await f.call("SealTransfer", [f.id, f.owner, transfer.View.id, record]);
  assert.equal((await f.rpc("Allocate", [f.id, f.owner])).status, 409);
  const reference = { session_id: f.id, sequence, digest: hash(JSON.stringify(record)) };
  assert.equal((await f.rpc("VerifyTransferTail", [f.id, transfer.View.id, target, reference])).status, 409);
  const [members] = await f.call("Allocations", [f.id, 0, 128]);
  await f.call("PublishCoverage", [f.owner, { Session: f.id, First: 1, Last: integer(sequence), Key: "immutable/transfer",
    Digest: hash("manifest"), Bytes: 512, Generation: 1, Records: members }]);
  assert.equal((await f.rpc("VerifyTransferRuns", [f.id, transfer.View.id, target, "", []])).status, 409);
  await f.call("VerifyTransferTail", [f.id, transfer.View.id, target, reference]);
  assert.equal((await f.rpc("VerifyTransferRuns", [f.id, transfer.View.id, target, "", []])).status, 409, "retained run cannot be skipped");
  const runs = await f.call("TransferRuns", [f.id, transfer.View.id, "", 4]);
  assert.equal(runs.length, 1);
  assert.equal(runs[0].View.id, operation.View.run_id);
  await f.call("VerifyTransferRuns", [f.id, transfer.View.id, target, "", [operation.View.run_id]]);
  const completed = await f.call("VerifyTransferRuns", [f.id, transfer.View.id, target, operation.View.run_id, []]);
  assert.equal(completed.View.phase, "transferred");
  assert.equal(String(completed.View.verified_runs), "1");
  assert.equal((await f.rpc("Allocate", [f.id, f.owner])).status, 409);
  const owner = await f.call("ClaimOwner", [f.id, target]);
  assert.equal(String(owner.Epoch), "2");
  await f.call("ClaimDispatch", [f.id, owner, operation.View.id, "sender", "claim"]);
  assert.equal((await f.rpc("ClaimDispatch", [f.id, owner, operation.View.id, "sender", "claim"])).status, 409);
  assert.equal((await f.call("BeginTransfer", [f.id, request])).View.phase, "transferred");
  assert.equal((await f.rpc("CancelTransfer", [f.id, transfer.View.id])).status, 409);
});

test("actual D1 rejects opaque transfer and distinguishes unplanned owner recovery", async t => {
  const f = await fixture(t);
  const operation = await f.prepare("opaque-operation");
  await f.call("BeginDecision", [f.id, f.owner, operation.View.run_id, 0, hash("request"), "source-process", false]);
  const target = "transfer-target";
  await f.call("RegisterNode", [target, 60000]);
  await f.call("ConfigureNode", [target, "http://target", f.custody, ["storage:retained"]]);
  const request = { request_id: "move", owner_epoch: "1", target_node: target };
  assert.equal((await f.rpc("BeginTransfer", [f.id, request])).status, 422);
  await f.call("Accept", [{ Tenant: "tenant", Subject: "operator" }, f.id,
    { request_id: "cancel-opaque", data: { kind: "control", control: "cancel" } }]);
  const transfer = await f.call("BeginTransfer", [f.id, request]);
  await f.call("FinishStep", [f.id, f.owner]);
  await f.call("RetireNode", [f.owner.Node]);
  assert.deepEqual(await f.call("Pending", [target, 16]), [f.id]);
  await f.call("ClaimOwner", [f.id, target]);
  const lost = await f.call("Transfer", [f.id, transfer.View.id]);
  assert.equal(lost.View.phase, "failed");
  assert.equal(lost.View.error.code, "uncertain");
});

test("actual D1 binds harness decisions to their original admitted process and repeatability", async t => {
  const f = await fixture(t);
  for (const repeatable of [false, true]) {
    const name = repeatable ? "checkpointed" : "process-bound";
    const command = await f.call("Accept", [{ Tenant: "tenant", Subject: "operator" }, f.id,
      { request_id: name, data: { kind: "message", text: name } }]);
    const run = stableID("run", f.id, command.id);
    await f.call("ApplyInput", [f.id, { Command: command.id, Owner: f.owner, Sequence: 1, Run: true, Disposition: "", StateKey: "" }]);
    const decision = { kind: "complete", result: "answer", checkpoint: {} };
    assert.equal((await f.rpc("SaveHarnessDecision", [f.id, f.owner, run, 0, decision, "unadmitted"])).status, 409);
    await f.call("BeginDecision", [f.id, f.owner, run, 0, hash(name), "original", repeatable]);
    const retry = await f.rpc("BeginDecision", [f.id, f.owner, run, 0, hash(name), "replacement", true]);
    assert.equal(retry.status, repeatable ? 200 : 422, retry.text);
    const divergence = await f.rpc("BeginDecision", [f.id, f.owner, run, 0, hash("changed"), "replacement", true]);
    assert.equal(divergence.status, 500, divergence.text);
    assert.equal(divergence.data.code, "corrupt");
    const process = repeatable ? "replacement" : "original";
    assert.equal((await f.call("Runs", [f.id, 64, ""])).find(value => value.View.id === run).HarnessProcess, process);
    assert.equal((await f.rpc("SaveHarnessDecision", [f.id, f.owner, run, 0, decision, "different"])).status, 409);
    await f.call("SaveHarnessDecision", [f.id, f.owner, run, 0, decision, process]);
  }
});

test("actual D1 handoff fences accepted and blocked work and reconciles without undoing later transitions", async t => {
  const first = { name: "fixture", version: "1" };
  const second = { name: "specialist", version: "1" };
  const f = await fixture(t, { config: { harness_transitions: [first, second] },
    capabilities: [first, second].map(value => "harness:" + stableID(value.name, value.version, "fixture")) });
  const request = { request_id: "forward", revision: "1", harness: { version: "1", name: "specialist" } };
  const actor = { Tenant: "tenant", Subject: "operator" };
  await f.call("Accept", [actor, f.id, { request_id: "pending", data: { kind: "message", text: "old harness" } }]);
  assert.equal((await f.rpc("Handoff", [f.id, request])).status, 409);
  await f.call("Accept", [actor, f.id, { request_id: "cancel-pending", data: { kind: "control", control: "cancel" } }]);
  const moved = await f.call("Handoff", [f.id, request]);
  assert.equal(moved.View.config.harness.name, "specialist");
  assert.equal((await f.call("Handoff", [f.id, request])).View.revision, moved.View.revision);
  const operation = await f.prepare("blocked");
  await f.call("AdvanceRun", [f.id, f.owner, operation.View.run_id, 0, "blocked", null, { code: "unsupported", message: "opaque continuation" }]);
  const back = { request_id: "back", revision: moved.View.revision, harness: first };
  assert.equal((await f.rpc("Handoff", [f.id, back])).status, 409);
  await f.call("Accept", [actor, f.id, { request_id: "cancel-blocked", data: { kind: "control", control: "cancel" } }]);
  const returned = await f.call("Handoff", [f.id, back]);
  const replay = await f.call("Handoff", [f.id, request]);
  assert.equal(replay.View.revision, returned.View.revision);
  assert.equal(replay.View.config.harness.name, "fixture");
  assert.equal((await f.rpc("Handoff", [f.id, { ...request, revision: "0" }])).status, 409);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM harness_handoffs WHERE session_id=?").bind(f.id).first()).count, 2);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM commands WHERE session_id=? AND kind='journal' AND json_extract(payload,'$.name')='harness.handed_off'").bind(f.id).first()).count, 2);
});

test("actual D1 binding authenticates only finite fresh domain requests", async t => {
  const f = await fixture(t);
  assert.equal((await f.rpc("Clock", [], { headers: { "X-Brain-Control-Signature": "0".repeat(64) } })).status, 401);
  assert.equal((await f.rpc("Clock", [], { time: Date.now() - 120000 })).status, 401);
  assert.equal((await f.rpc("Clock", [], { body: '{"arguments":["1"]}' })).status, 401);
  assert.equal((await f.rpc("SQL", ["DELETE FROM sessions"])).status, 404);
  assert.equal((await f.rpc("Clock", [1])).status, 400);
  const mismatch = await f.rpc("ConfigureNode", [f.owner.Node, "http://retained", hash("different-key"), ["storage:retained"]]);
  assert.equal(mismatch.status, 403);
  assert.match(mismatch.text, /original retained custody/u);
  assert.equal((await f.call("Session", ["tenant", f.id])).View.id, f.id);
});

test("actual D1 reconciles proven scope rollback without losing original receipts or written positions", async t => {
  const f = await fixture(t);
  const original = await f.prepare("contended-result");
  await f.call("ClaimDispatch", [f.id, f.owner, original.View.id, f.owner.Node, "original-claim"]);
  const sequence = await f.call("Allocate", [f.id, f.owner]);
  const digest = hash("original retained result");
  const receipt = { request_id: "original-receipt", outcome: "completed", result: { original: true }, usage: { input_tokens: 7 } };
  const requests = [
    ["RecordReceipt", [f.id, original.View.id, receipt, 0, ""]],
    ["Written", [f.id, f.owner, sequence, digest, 64]],
    ...Array.from({ length: 6 }, (_, index) => ["Accept", [{ Tenant: "tenant", Subject: "operator" }, f.id,
      { request_id: `concurrent-${index}`, data: { kind: "message", text: `accepted ${index}` } }]]),
  ];
  const responses = await Promise.all(requests.map(([method, args]) => f.rpc(method, args)));
  for (let index = 0; index < responses.length; index++) {
    assert.equal(responses[index].status, 200, `${requests[index][0]}: ${responses[index].text}`);
  }
  assert.equal((await f.call("LocateRecord", [f.id, sequence])).Status, "written");
  assert.equal((await f.call("Operation", [f.id, original.View.id])).View.status, "completed");
  await f.call("BindResult", [f.id, f.owner, original.View.id, sequence, digest]);
  await f.call("RecordReceipt", [f.id, original.View.id, receipt, 0, ""]);
  assert.equal((await f.rpc("ClaimDispatch", [f.id, f.owner, original.View.id, f.owner.Node, "original-claim"])).status, 409);
  const commands = await f.database.prepare("SELECT COUNT(*) AS count FROM commands WHERE session_id=? AND request_key LIKE 'concurrent-%'")
    .bind(f.id).first();
  assert.equal(commands.count, 6);
  assert.deepEqual(await f.database.prepare("SELECT active,reserved,calls FROM budgets WHERE id=?").bind(f.id).first(),
    { active: 0, reserved: 0, calls: 1 });
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM d1_scope_conflicts").first()).count, 0);
});

test("actual D1 atomic claims issue one permission and preserve exact original evidence", async t => {
  const f = await fixture(t);
  const operation = await f.prepare();
  const claims = await Promise.all(Array.from({ length: 24 }, () =>
    f.rpc("ClaimDispatch", [f.id, f.owner, operation.View.id, f.owner.Node, "same-claim"])));
  assert.equal(claims.filter(value => value.status === 200).length, 1, JSON.stringify(claims));
  assert(claims.every(value => [200, 409, 429].includes(value.status)), JSON.stringify(claims));
  assert.equal((await f.rpc("ClaimDispatch", [f.id, f.owner, operation.View.id, f.owner.Node, "same-claim"])).status, 409);
  const retained = await f.call("Operation", [f.id, operation.View.id]);
  assert.equal(retained.Arguments.nested.number.toString(), "9007199254740993");
  assert.equal(retained.Arguments.escaped, "quote\"slash\\");
  const budget = await f.database.prepare("SELECT active,reserved FROM budgets WHERE id=?").bind(f.id).first();
  assert.equal(budget.active, 1);
  assert(budget.reserved > 0);
  await f.database.prepare("UPDATE operations SET deadline_ms=0 WHERE id=?").bind(operation.View.id).run();
  assert.equal((await f.call("ExpireOperations", [8])).toString(), "1");
  assert.equal((await f.call("Operation", [f.id, operation.View.id])).View.status, "unknown");
  const unknownBudget = await f.database.prepare("SELECT active,reserved FROM budgets WHERE id=?").bind(f.id).first();
  assert.deepEqual(unknownBudget, budget);
  const unknownSequence = await f.call("Allocate", [f.id, f.owner]);
  await f.call("Written", [f.id, f.owner, unknownSequence, hash("original unknown evidence"), 32]);
  await f.call("BindResult", [f.id, f.owner, operation.View.id, unknownSequence, hash("original unknown evidence")]);
  assert.deepEqual(await f.database.prepare("SELECT active,reserved FROM budgets WHERE id=?").bind(f.id).first(), budget);
  const receipt = { request_id: "original-receipt", outcome: "completed", result: { original: true }, usage: { input_tokens: 3 } };
  const completed = await f.call("RecordReceipt", [f.id, operation.View.id, receipt, 0, ""]);
  assert.equal(completed.View.status, "completed");
  assert.equal(Object.hasOwn(completed.View.usage, "output_tokens"), false);
  await f.call("RecordReceipt", [f.id, operation.View.id, receipt, 0, ""]);
  assert.deepEqual(await f.database.prepare("SELECT active,reserved FROM budgets WHERE id=?").bind(f.id).first(), { active: 0, reserved: budget.reserved });
  const sequence = await f.call("Allocate", [f.id, f.owner]);
  const digest = hash("original completed evidence");
  assert.equal((await f.rpc("BindResult", [f.id, f.owner, operation.View.id, sequence, digest])).status, 409);
  await f.call("Written", [f.id, f.owner, sequence, digest, 32]);
  await f.call("BindResult", [f.id, f.owner, operation.View.id, sequence, digest]);
  await f.call("BindResult", [f.id, f.owner, operation.View.id, sequence, digest]);
  assert.deepEqual(await f.database.prepare("SELECT active,reserved FROM budgets WHERE id=?").bind(f.id).first(), { active: 0, reserved: 0 });
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM d1_guards").first()).count, 0);
});

test("actual D1 bounds provider metadata before storing a known completion", async t => {
  const f = await fixture(t);
  const prepared = await f.prepare();
  await f.call("ClaimDispatch", [f.id, f.owner, prepared.View.id, f.owner.Node, "claim"]);
  const provider = { details: "u".repeat(1100000) };
  const receipt = { request_id: "large-metadata", outcome: "completed", result: { delivered: true }, usage: { input_tokens: 9, provider } };
  const completed = await f.call("RecordReceipt", [f.id, prepared.View.id, receipt, 0, ""]);
  assert.equal(completed.View.status, "completed");
  assert.equal(completed.View.result_available, true);
  assert.equal(completed.View.result.delivered, true);
  assert.equal(String(completed.View.usage.input_tokens), "9");
  assert.equal(Object.hasOwn(completed.View.usage, "output_tokens"), false);
  assert.deepEqual(completed.View.usage.provider, {});
  assert.equal(completed.View.usage.provider_unavailable.digest, hash(JSON.stringify(provider)));
  assert.equal(String(completed.View.usage.provider_unavailable.bytes), String(Buffer.byteLength(JSON.stringify(provider))));
  const stored = await f.database.prepare("SELECT length(CAST(result_body AS BLOB)) AS receipt,length(CAST(usage AS BLOB)) AS usage FROM operations WHERE id=?").bind(prepared.View.id).first();
  assert(stored.receipt < 49152 && stored.usage < 49152);
  const duplicate = await f.call("RecordReceipt", [f.id, prepared.View.id, receipt, 0, ""]);
  assert.equal(duplicate.View.revision, completed.View.revision);
});

test("actual D1 rolls back changed custody and fences positions beyond JavaScript integer precision", async t => {
  const f = await fixture(t);
  await f.database.prepare("UPDATE sessions SET sequence=CAST(? AS INTEGER),epoch=CAST(? AS INTEGER) WHERE id=?")
    .bind("9007199254740993", "9007199254740997", f.id).run();
  const owner = { Node: f.owner.Node, Epoch: integer("9007199254740997") };
  const sequence = await f.call("Allocate", [f.id, owner]);
  assert.equal(sequence.toString(), "9007199254740994");
  await f.call("RetireNode", [owner.Node]);
  await f.call("RegisterNode", ["successor", 60000]);
  await f.call("ConfigureNode", ["successor", "http://successor", f.custody, ["storage:retained"]]);
  const successor = await f.call("ClaimOwner", [f.id, "successor"]);
  assert.equal(successor.Epoch.toString(), "9007199254740998");
  assert.equal((await f.rpc("Allocate", [f.id, owner])).status, 409);
  assert.equal((await f.rpc("SealUnused", [f.id, successor, sequence])).status, 409);
  const [allocations, high] = await f.call("Allocations", [f.id, integer("9007199254740993"), 32]);
  assert.equal(high.toString(), "9007199254740994");
  assert.equal(allocations[0].Sequence.toString(), "9007199254740994");
  assert.equal(allocations[0].Status, "unresolved");
  assert.equal((await f.call("Allocate", [f.id, successor])).toString(), "9007199254740995");
  const changed = await f.rpc("ConfigureNode", ["successor", "http://successor", f.custody, ["storage:empty"]]);
  assert.equal(changed.status, 409);
  const capabilities = await f.database.prepare("SELECT capability FROM node_capabilities WHERE node_id=?").bind("successor").all();
  assert.deepEqual(capabilities.results, [{ capability: "storage:retained" }]);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM d1_guards").first()).count, 0);
});

test("actual D1 publishes 128 exact members within its bounded batch and rolls back invalid membership", async t => {
  const f = await fixture(t);
  const members = Array.from({ length: 128 }, (_, index) => ({ Sequence: index + 1, Digest: hash("record:" + index) }));
  await f.database.prepare(`INSERT INTO allocations(session_id,sequence,owner,epoch,status,digest,bytes)
SELECT ?,json_extract(value,'$.Sequence'),?,1,'written',json_extract(value,'$.Digest'),64 FROM json_each(?)`)
    .bind(f.id, f.owner.Node, JSON.stringify(members)).run();
  await f.database.prepare("UPDATE sessions SET sequence=128 WHERE id=?").bind(f.id).run();
  const coverage = { Session: f.id, First: 1, Last: 128, Key: "immutable/chunk", Digest: hash("manifest"), Bytes: 8192, Generation: 1, Records: members };
  const invalid = { ...coverage, Records: members.map((value, index) => index === 64 ? { ...value, Digest: hash("wrong") } : value) };
  assert.equal((await f.rpc("PublishCoverage", [f.owner, invalid])).status, 409);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM coverage").first()).count, 0);
  await f.call("PublishCoverage", [f.owner, coverage]);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM allocations WHERE archive_key=?").bind(coverage.Key).first()).count, 128);
  assert.equal((await f.call("Coverage", [f.id, 64, 8]))[0].Key, coverage.Key);
  assert.equal((await f.rpc("PublishCoverage", [f.owner, { ...coverage, Records: [...members, { Sequence: 129, Digest: hash("extra") }] }])).status, 400);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM d1_guards").first()).count, 0);
});

test("the Worker rejects oversized values before the actual D1 binding can store them", async t => {
  const f = await fixture(t);
  const value = { id: "oversized", body: "x".repeat(1048577) };
  assert.equal((await f.rpc("PutPackage", ["limits", value])).status, 429);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM packages WHERE tenant='limits'").first()).count, 0);
  const oversizedBody = await f.rpc("PutPackage", ["limits", { id: "oversized-body", body: "x".repeat(2097153) }]);
  assert.equal(oversizedBody.status, 400);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM packages WHERE tenant='limits'").first()).count, 0);
});

test("actual D1 rechecks a revoked authentication snapshot inside the atomic mutation", async t => {
  const f = await fixture(t);
  const operation = await f.prepare("not-authorized");
  const grant = await f.call("Grant", [f.id, { request_id: "browser", subject: "browser",
    permissions: ["input:write", "operation:claim"], expires_ms: Date.now() + 60000 }]);
  const authority = { Participant: grant.ID, Session: grant.Session, Tenant: grant.Tenant };
  await f.call("RevokeParticipant", [f.id, grant.ID]);
  const denied = await f.rpc("ClaimDispatch", [f.id, f.owner, operation.View.id, f.owner.Node, "stale"], { authority });
  assert.equal(denied.status, 401, denied.text);
  assert.equal((await f.call("Operation", [f.id, operation.View.id])).View.status, "prepared");
  const state = await f.rpc("PutDocument", [f.id, "state", "denied", { request_id: "denied", revision: "0", value: true }], { authority });
  assert.equal(state.status, 401, state.text);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM documents").first()).count, 0);
  const self = await f.call("Grant", [f.id, { request_id: "self", subject: "browser", permissions: ["participant:revoke"], expires_ms: Date.now() + 60000 }]);
  assert.equal((await f.rpc("RevokeParticipant", [f.id, self.ID],
    { authority: { Participant: self.ID, Session: self.Session, Tenant: self.Tenant } })).status, 200);
  assert.equal((await f.call("Participant", [self.ID])).Revoked, true);
});

test("actual D1 preserves omitted optional action and event authority during delegation", async t => {
  const f = await fixture(t);
  const config = structuredClone(f.config);
  config.connections = [{ name: "worker", kind: "application", provider: "application", ownership: "borrowed" }];
  config.actions = [{ name: "deliver", description: "Deliver once", connection: "worker",
    input_schema: { type: "object" }, max_output_bytes: 8192, timeout_ms: 30000 }];
  config.events = [{ name: "changed", version: "1", schema: { type: "object" }, disposition: "context" }];
  const parent = await f.call("Create", ["tenant", { request_id: "optional-parent", config }, "", "", []]);
  const child = await f.call("Create", ["tenant", { request_id: "optional-child", config }, parent.View.id, parent.Budget, []]);
  assert.deepEqual(child.View.config.actions, parent.View.config.actions);
  assert.deepEqual(child.View.config.events, parent.View.config.events);
  const strengthened = structuredClone(config);
  strengthened.actions[0].approval = true;
  strengthened.events[0].sources = ["producer"];
  await f.call("Create", ["tenant", { request_id: "strengthened-child", config: strengthened }, parent.View.id, parent.Budget, []]);
  const broadened = structuredClone(config);
  broadened.actions[0].connection = "different-worker";
  assert.equal((await f.rpc("Create", ["tenant", { request_id: "different-worker", config: broadened }, parent.View.id, parent.Budget, []])).status, 403);
});

test("actual D1 bounds deep budget ancestry and rolls back the parent link with failed child admission", async t => {
  const f = await fixture(t);
  const narrow = structuredClone(f.config);
  narrow.limits.input_bytes = 128;
  const invalid = await f.rpc("Create", ["tenant", { request_id: "invalid-child", config: narrow, message: "x".repeat(129) }, f.id, f.id, ["storage:retained"]]);
  assert.equal(invalid.status, 400, invalid.text);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM sessions WHERE create_key='invalid-child'").first()).count, 0);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM commands").first()).count, 0, "parent audit rolls back with its child");
  let parent = f.id;
  for (let depth = 1; depth <= 8; depth++) {
    const child = await f.call("Create", ["tenant", { request_id: "depth-" + depth, config: f.config }, parent, f.id, ["storage:retained"]]);
    parent = child.View.id;
  }
  const rejected = await f.rpc("Create", ["tenant", { request_id: "too-deep", config: f.config }, parent, f.id, ["storage:retained"]]);
  assert.equal(rejected.status, 403, rejected.text);
  const owner = await f.call("ClaimOwner", [parent, f.owner.Node]);
  const command = await f.call("Accept", [{ Tenant: "tenant", Subject: "operator" }, parent,
    { request_id: "deep-work", data: { kind: "message", text: "deep-work" } }]);
  await f.call("ApplyInput", [parent, { Command: command.id, Owner: owner, Sequence: 1, Run: true, Disposition: "", StateKey: "" }]);
  const sequence = await f.call("Allocate", [parent, owner]);
  const digest = hash("deep-intent");
  await f.call("Written", [parent, owner, sequence, digest, 64]);
  const operation = await f.call("Prepare", [owner, {
    ID: "deep-operation", Session: parent, Run: stableID("run", parent, command.id), StepKey: "0", Kind: "model", Action: "generate",
    Arguments: { messages: [{ role: "user", content: "deep-work" }], actions: [] },
    Recipient: "model", OutputLimit: 8192, Timeout: 30000, Generation: "", Approval: false, RetryOf: "",
    IntentSequence: sequence, IntentDigest: digest,
  }]);
  assert.equal((await f.call("Budget", [f.id])).active_operations.toString(), "1");
  await f.call("Accept", [{ Tenant: "tenant", Subject: "operator" }, f.id,
    { request_id: "pause-descendants", data: { kind: "control", control: "pause" } }]);
  const paused = await f.rpc("ClaimDispatch", [parent, owner, operation.View.id, owner.Node, "paused"]);
  assert.equal(paused.status, 409, paused.text);
  await f.call("Accept", [{ Tenant: "tenant", Subject: "operator" }, f.id,
    { request_id: "resume-descendants", data: { kind: "control", control: "resume" } }]);
  await f.database.prepare("UPDATE operations SET deadline_ms=0 WHERE id=?").bind(operation.View.id).run();
  assert.equal((await f.call("ExpireOperations", [64])).toString(), "1", "nine enclosing pools fit one actual D1 batch");
  for (const identifier of [f.id, parent]) {
    const budget = await f.call("Budget", [identifier]);
    assert.equal(budget.calls.toString(), "1");
    assert.equal(budget.active_operations.toString(), "0");
    assert.equal(budget.reserved_tokens, "0");
    assert.equal(budget.incomplete_usages.toString(), "0");
  }
  await f.call("Accept", [{ Tenant: "tenant", Subject: "operator" }, f.id,
    { request_id: "cancel-descendants", data: { kind: "control", control: "cancel" } }]);
  const escape = await f.rpc("Create", ["tenant", { request_id: "cannot-escape", config: f.config }, parent, f.id, ["storage:retained"]]);
  assert.equal(escape.status, 409, escape.text);
  let closed = 0;
  for (let batch = 0; batch < 3; batch++) {
    const page = await f.call("PropagateControls", [4]);
    assert(page.length <= 4);
    closed += page.length;
  }
  assert.equal(closed, 8);
  assert.equal((await f.call("Session", ["tenant", parent])).View.lifecycle, "closed");
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM d1_guards").first()).count, 0);
});

test("actual D1 binds completed runs only to written bytes matching the saved result", async t => {
  const f = await fixture(t);
  const command = await f.call("Accept", [{ Tenant: "tenant", Subject: "operator" }, f.id,
    { request_id: "completion", data: { kind: "message", text: "work" } }]);
  const run = stableID("run", f.id, command.id);
  await f.call("ApplyInput", [f.id, { Command: command.id, Owner: f.owner, Sequence: 1, Run: true, Disposition: "", StateKey: "" }]);
  await f.call("SaveDecision", [f.id, f.owner, run, 0, { kind: "complete", result: { answer: 42 }, checkpoint: {} }]);
  assert.equal((await f.rpc("AdvanceRun", [f.id, f.owner, run, 0, "completed", null, null])).status, 400);
  const sequence = String(await f.call("Allocate", [f.id, f.owner]));
  const record = { session_id: f.id, sequence, run_id: run, kind: "run.completed", time_ms: 1, data: { result: { answer: 42 } } };
  assert.equal((await f.rpc("CompleteRun", [f.id, f.owner, run, 0, record])).status, 409);
  const wrong = { ...record, data: { result: "another answer" } };
  await f.call("Written", [f.id, f.owner, integer(sequence), hash(JSON.stringify(wrong)), Buffer.byteLength(JSON.stringify(wrong))]);
  assert.equal((await f.rpc("CompleteRun", [f.id, f.owner, run, 0, wrong])).status, 409);
  const next = String(await f.call("Allocate", [f.id, f.owner]));
  record.sequence = next;
  const digest = hash(JSON.stringify(record));
  await f.call("Written", [f.id, f.owner, integer(next), digest, Buffer.byteLength(JSON.stringify(record))]);
  await f.call("CompleteRun", [f.id, f.owner, run, 0, record]);
  await f.call("CompleteRun", [f.id, f.owner, run, 0, record]);
  const [completed] = await f.call("Runs", [f.id, 8, ""]);
  assert.equal(completed.View.status, "completed");
  assert.deepEqual(completed.View.completion, { session_id: f.id, sequence: next, digest });
  assert.equal((await f.rpc("CompleteRun", [f.id, f.owner, run, 0, { ...record, data: wrong.data }])).status, 409);
});

test("actual D1 reconciles eight stable fork slots across ownership without hiding partial admission", async t => {
  const f = await fixture(t);
  const command = await f.call("Accept", [{ Tenant: "tenant", Subject: "operator" }, f.id,
    { request_id: "fork", data: { kind: "message", text: "work" } }]);
  const run = stableID("run", f.id, command.id);
  await f.call("ApplyInput", [f.id, { Command: command.id, Owner: f.owner, Sequence: 1, Run: true, Disposition: "", StateKey: "" }]);
  const delegations = Array.from({ length: 8 }, (_, index) => ({ request_id: "slot-" + index, config: f.config, message: "child-" + index, context: [] }));
  const oversized = await f.rpc("SaveDecision", [f.id, f.owner, run, 0,
    { kind: "fork", delegations: [...delegations, delegations[0]], checkpoint: {} }]);
  assert.equal(oversized.status, 400, oversized.text);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM child_groups").first()).count, 0);
  await f.call("SaveDecision", [f.id, f.owner, run, 0, { kind: "fork", delegations, checkpoint: {} }]);
  const request = index => ({ request_id: "caller-key", config: f.config, message: delegations[index].message });
  const first = await f.call("Delegate", [f.id, f.owner, run, 0, 0, request(0), ["storage:retained"]]);
  const partial = await f.call("Children", [f.id, run, 0]);
  assert.equal(partial.ready, false);
  assert.equal(String(partial.expected), "8");
  assert.equal(partial.children.length, 1);
  assert.equal(partial.children[0].availability, "pending");
  assert.equal((await f.rpc("AdvanceRun", [f.id, f.owner, run, 0, "waiting", { kind: "children", id: "0" }, null])).status, 409);
  await f.call("RetireNode", [f.owner.Node]);
  await f.call("RegisterNode", ["fork-successor", 60000]);
  await f.call("ConfigureNode", ["fork-successor", "http://successor", f.custody, ["storage:retained"]]);
  const owner = await f.call("ClaimOwner", [f.id, "fork-successor"]);
  const again = await f.call("Delegate", [f.id, owner, run, 0, 0, request(0), ["storage:retained"]]);
  assert.equal(again.View.id, first.View.id);
  const changed = await f.rpc("Delegate", [f.id, owner, run, 0, 1, { ...request(1), message: "different decision" }, ["storage:retained"]]);
  assert.equal(changed.status, 409, changed.text);
  for (let index = 1; index < 8; index++) await f.call("Delegate", [f.id, owner, run, 0, index, request(index), ["storage:retained"]]);
  await f.call("AdvanceRun", [f.id, owner, run, 0, "waiting", { kind: "children", id: "0" }, null]);
  const waiting = await f.call("Children", [f.id, run, 0]);
  assert.equal(waiting.children.length, 8);
  assert.equal(waiting.ready, false);
  for (const child of waiting.children) {
    await f.call("Accept", [{ Tenant: "tenant", Subject: "operator" }, child.session_id,
      { request_id: "cancel-original-input", data: { kind: "control", control: "cancel" } }]);
  }
  const settled = await f.call("Children", [f.id, run, 0]);
  assert.equal(settled.ready, true);
  assert(settled.children.every(child => child.status === "cancelled" && child.availability === "none"));
  assert.equal((await f.call("RunnableRuns", [f.id, 8]))[0].View.id, run);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM d1_guards").first()).count, 0);
});

test("actual D1 requires the original live saved decision when a harness admits a child", async t => {
  const f = await fixture(t);
  const command = await f.call("Accept", [{ Tenant: "tenant", Subject: "operator" }, f.id,
    { request_id: "parent-decision", data: { kind: "message", text: "work" } }]);
  const run = stableID("run", f.id, command.id);
  await f.call("ApplyInput", [f.id, { Command: command.id, Owner: f.owner, Sequence: 1, Run: true, Disposition: "", StateKey: "" }]);
  const request = { request_id: "child-at-boundary", config: f.config, message: "work" };
  await f.call("SaveDecision", [f.id, f.owner, run, 0, { kind: "delegate",
    delegation: { request_id: "helper", config: f.config, message: "work", context: [] }, checkpoint: {} }]);
  await f.call("Delegate", [f.id, f.owner, run, 0, 0, request, ["storage:retained"]]);
  await f.call("Accept", [{ Tenant: "tenant", Subject: "operator" }, f.id,
    { request_id: "cancel-boundary", data: { kind: "control", control: "cancel" } }]);
  const denied = await f.rpc("Delegate", [f.id, f.owner, run, 0, 0, { ...request, request_id: "another-child" }, ["storage:retained"]]);
  assert.equal(denied.status, 409, denied.text);
  assert.equal((await f.database.prepare("SELECT COUNT(*) AS count FROM sessions").first()).count, 2);
});
