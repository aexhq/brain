import {
  Mutation, big, capability, clock, decode, digest, id, identifiers, integer, maximumInteger,
  need, one, raw, result, rows, run, runColumns, safe,
} from "./domain.mjs";
import { bytes, pageRows, runBytes } from "./pages.mjs";
import { equal } from "./budgets.mjs";

const active = "phase IN ('draining','verifying')";
const retainedRuns = "status IN ('queued','active','waiting','paused','blocked')";
const drained = `NOT EXISTS(SELECT 1 FROM operations o WHERE o.session_id=t.session_id AND
(o.status='dispatched' OR (o.status IN ('completed','failed','unknown') AND o.result_sequence=0)))
AND NOT EXISTS(SELECT 1 FROM allocations a WHERE a.session_id=t.session_id AND a.status IN ('reserved','unresolved'))`;
const archived = `NOT EXISTS(SELECT 1 FROM allocations a WHERE a.session_id=t.session_id AND
a.sequence<=t.through_sequence AND a.status='written' AND a.archive_key='')`;
const columns = `t.id,t.session_id,t.source_node,CAST(t.source_epoch AS TEXT) AS source_epoch,t.target_node,t.phase,
CAST(t.through_sequence AS TEXT) AS through_sequence,t.checkpoint_digest,t.run_cursor,t.tail_verified,
t.verified_runs,t.error,CASE WHEN ${drained} THEN 1 ELSE 0 END AS drained,CASE WHEN ${archived} THEN 1 ELSE 0 END AS archived,
CASE WHEN EXISTS(SELECT 1 FROM nodes WHERE id=t.target_node AND lease>${clock} AND draining=0) THEN 1 ELSE 0 END AS target_live`;

function transfer(value) {
  return { View: { id: value.id, session_id: value.session_id, source_node: value.source_node, source_epoch: value.source_epoch,
    target_node: value.target_node, phase: value.phase, through: value.through_sequence, verified_runs: value.verified_runs,
    ...(big(value.through_sequence) > 0n ? { checkpoint: {
      session_id: value.session_id, sequence: value.through_sequence, digest: value.checkpoint_digest,
    } } : {}), ...(value.error ? { error: raw(value.error) } : {}) },
  Cursor: value.run_cursor, TailVerified: value.tail_verified !== 0, Drained: value.drained !== 0, Archived: value.archived !== 0,
  TargetLive: value.target_live !== 0 };
}

async function selected(db, identifier, transferID) {
  return db instanceof Mutation
    ? db.one("SELECT " + columns + " FROM ownership_transfers t WHERE t.session_id=? AND t.id=?", [identifier, transferID])
    : one(db, "SELECT " + columns + " FROM ownership_transfers t WHERE t.session_id=? AND t.id=?", [identifier, transferID]);
}

async function target(m, current, node) {
  const predicate = `EXISTS(SELECT 1 FROM nodes n WHERE n.id=? AND n.lease>${clock} AND n.draining=0
AND NOT EXISTS(SELECT 1 FROM session_requirements r WHERE r.session_id=?
AND NOT EXISTS(SELECT 1 FROM node_capabilities c WHERE c.node_id=n.id AND c.capability=r.capability)))`;
  const valid = await m.one("SELECT CASE WHEN " + predicate + " THEN 1 ELSE 0 END AS valid", [node, current.id]);
  need(valid.valid === 1, "unsupported", "target incarnation lacks a live compatible lease or retained resources");
  m.guard(predicate, [node, current.id]);
  const started = await m.one(`SELECT COUNT(*) AS count,COALESCE(SUM(CASE WHEN decision_repeatable=0 THEN 1 ELSE 0 END),0) AS opaque
FROM runs WHERE session_id=? AND ${retainedRuns} AND harness_process<>''`, [current.id]);
  need(started.opaque === 0, "unsupported", "a process-bound harness has unsettled work; resolve or cancel it before transfer");
  if (started.count !== 0) {
    const config = decode(current.config);
    const required = await capability("continuation", config.harness.name, config.harness.version, config.model.provider);
    const available = await m.one("SELECT COUNT(*) AS count FROM node_capabilities WHERE node_id=? AND capability=?", [node, required]);
    need(available.count === 1, "unsupported", "target cannot resume the admitted harness checkpoint");
  }
}

async function guard(m, identifier, transferID, node) {
  const current = await m.session(identifier);
  const value = await selected(m, identifier, transferID);
  need(["draining", "verifying"].includes(value.phase), "conflict", "ownership transfer is already settled");
  need(current.owner === value.source_node && current.epoch === value.source_epoch &&
    [value.source_node, value.target_node].includes(node), "conflict", "transfer owner or assigned participant changed");
  await m.owner(identifier, { Node: current.owner, Epoch: integer(current.epoch) });
  need(current.lifecycle === "open", "conflict", "transfer session is no longer open");
  await target(m, current, value.target_node);
  return { current, value };
}

function after(m, identifier, transferID) {
  return result(m, "SELECT " + columns + " FROM ownership_transfers t WHERE t.session_id=? AND t.id=?", [identifier, transferID], transfer);
}

export const transferHandlers = {
  async Nodes(db, [cursor, limit]) {
    return (await pageRows(db, "id,endpoint,lease,draining", "nodes", "id>?", "id", bytes("endpoint"), limit, [cursor]))
      .map(value => ({ id: value.id, endpoint: value.endpoint, lease_expires_ms: value.lease, draining: value.draining !== 0 }));
  },
  async Transfer(db, [identifier, transferID]) {
    return transfer(await selected(db, identifier, transferID));
  },
  async BeginTransfer(db, [identifier, request], text) {
    identifiers([request.request_id, request.target_node]);
    const transferID = await id("transfer", identifier, request.request_id);
    const hash = await digest(text[1]);
    const m = new Mutation(db);
    const current = await m.session(identifier);
    const prior = await m.one("SELECT request_hash FROM ownership_transfers WHERE session_id=? AND id=?", [identifier, transferID], true);
    if (prior) {
      need(prior.request_hash === hash, "conflict", "transfer request id has different arguments");
      return transfer(await selected(m, identifier, transferID));
    }
    need(current.epoch === request.owner_epoch && current.owner !== request.target_node && current.lifecycle === "open",
      "conflict", "transfer requires the expected live owner and a different target");
    await m.owner(identifier, { Node: current.owner, Epoch: integer(current.epoch) });
    await m.unfrozen(identifier);
    await target(m, current, request.target_node);
    m.change(`INSERT INTO ownership_transfers
(id,session_id,request_key,request_hash,source_node,source_epoch,target_node,updated_ms) VALUES(?,?,?,?,?,?,?,${clock})`,
    [transferID, identifier, request.request_id, hash, current.owner, current.epoch, request.target_node]);
    m.change("UPDATE sessions SET ready=1 WHERE id=?", [identifier], true);
    return after(m, identifier, transferID);
  },
  async PendingTransfers(db, [node, cursor, limit]) {
    return (await rows(db, `SELECT ${columns} FROM ownership_transfers t WHERE t.phase IN ('draining','verifying')
AND (t.source_node=? OR t.target_node=?) AND t.id>? ORDER BY t.id LIMIT ?`, [node, node, cursor, safe(limit, 1, 16)])).map(transfer);
  },
  async SealTransfer(db, [identifier, owner, transferID, record], text) {
    const m = new Mutation(db);
    const { current, value } = await guard(m, identifier, transferID, owner.Node);
    need(current.owner === owner.Node && current.epoch === String(owner.Epoch) &&
      value.source_node === owner.Node && value.phase === "draining" && value.drained !== 0,
    "conflict", "transfer has not drained its original work and allocations");
    const expected = { format: "brain-ownership-checkpoint/1", transfer_id: transferID,
      source_epoch: value.source_epoch, target_node: value.target_node };
    need(record.session_id === identifier && record.sequence === current.sequence && record.kind === "ownership.checkpoint" &&
      equal(record.data, expected) && !record.run_id && !record.operation_id,
    "conflict", "transfer checkpoint does not match its exact frozen boundary");
    const hash = await digest(text[3]);
    const written = await m.one("SELECT COUNT(*) AS count FROM allocations WHERE session_id=? AND sequence=? AND status='written' AND digest=?",
      [identifier, record.sequence, hash]);
    need(written.count === 1, "conflict", "transfer checkpoint has no matching written evidence");
    m.change(`UPDATE ownership_transfers SET phase='verifying',through_sequence=?,checkpoint_digest=?,updated_ms=${clock} WHERE id=?`,
      [record.sequence, hash, transferID], true);
    await m.commit();
  },
  async TransferRuns(db, [identifier, transferID, cursor, limit]) {
    const value = await selected(db, identifier, transferID);
    need(value.phase === "verifying" && value.run_cursor === cursor, "conflict", "transfer verification cursor changed");
    return (await pageRows(db, runColumns, "runs r JOIN commands c ON c.id=r.command_id",
      "r.session_id=? AND r.id>? AND r." + retainedRuns, "r.id", runBytes, safe(limit, 1, 4), [identifier, cursor])).map(run);
  },
  async VerifyTransferTail(db, [identifier, transferID, node, reference]) {
    const m = new Mutation(db);
    const { current, value } = await guard(m, identifier, transferID, node);
    need(value.phase === "verifying" && value.target_node === node && value.archived !== 0 && current.sequence === value.through_sequence &&
      reference.session_id === identifier && reference.sequence === value.through_sequence && reference.digest === value.checkpoint_digest,
    "conflict", "target cannot attest a different or uncovered checkpoint");
    m.change(`UPDATE ownership_transfers SET tail_verified=1,updated_ms=${clock} WHERE id=?`, [transferID], true);
    await m.commit();
  },
  async VerifyTransferRuns(db, [identifier, transferID, node, cursor, verified]) {
    need(Array.isArray(verified) && verified.length <= 4, "invalid", "transfer proof page exceeds its bound");
    const m = new Mutation(db);
    const { current, value } = await guard(m, identifier, transferID, node);
    need(value.phase === "verifying" && value.target_node === node && value.run_cursor === cursor &&
      value.tail_verified !== 0 && value.archived !== 0 && current.sequence === value.through_sequence,
    "conflict", "transfer checkpoint, proof cursor or archive coverage changed");
    const expected = (await m.rows(`SELECT id FROM runs WHERE session_id=? AND id>? AND ${retainedRuns} ORDER BY id LIMIT ?`,
      [identifier, cursor, Math.max(1, verified.length)])).map(value => value.id);
    need(equal(expected, verified), "conflict", "transfer proof skipped or changed a retained run");
    if (verified.length) {
      m.change(`UPDATE ownership_transfers SET run_cursor=?,verified_runs=verified_runs+?,updated_ms=${clock} WHERE id=?`,
        [verified.at(-1), verified.length, transferID], true);
    } else {
      need(big(current.epoch) < maximumInteger, "overloaded", "owner epoch exhausted");
      m.change(`UPDATE sessions SET owner=?,epoch=epoch+1,ready=1,ready_ms=${clock}
WHERE id=? AND owner=? AND epoch=? AND sequence=?`, [node, identifier, value.source_node, current.epoch, value.through_sequence], true);
      m.change(`UPDATE ownership_transfers SET phase='transferred',updated_ms=${clock} WHERE id=?`, [transferID], true);
    }
    return after(m, identifier, transferID);
  },
  async CancelTransfer(db, [identifier, transferID]) {
    const m = new Mutation(db);
    await m.session(identifier);
    const value = await selected(m, identifier, transferID);
    need(value.phase !== "transferred", "conflict", "completed ownership transfer cannot be undone");
    if (["draining", "verifying"].includes(value.phase)) {
      m.change(`UPDATE ownership_transfers SET phase='cancelled',updated_ms=${clock} WHERE id=?`, [transferID], true);
      m.change("UPDATE sessions SET ready=1 WHERE id=?", [identifier], true);
    }
    return after(m, identifier, transferID);
  },
  async FailTransfer(db, [identifier, transferID, node, failure], text) {
    const m = new Mutation(db);
    await m.session(identifier);
    const value = await selected(m, identifier, transferID);
    need([value.source_node, value.target_node].includes(node), "forbidden", "only an assigned transfer node may record its failure");
    m.change(`UPDATE ownership_transfers SET phase='failed',error=?,updated_ms=${clock} WHERE id=? AND ${active}
AND EXISTS(SELECT 1 FROM nodes WHERE id=? AND lease>${clock} AND draining=0)`, [text[3], transferID, node], true);
    m.change("UPDATE sessions SET ready=1 WHERE id=?", [identifier], true);
    await m.commit();
  },
};
