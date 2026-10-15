import {
  Mutation, authoritySubject, big, clock, decode, digest, encode, field, need, one, operation, operationColumns, raw, result, rows, safe, wire,
} from "./domain.mjs";
import { journal } from "./control.mjs";
import { operationBytes, pageRows } from "./pages.mjs";
import { releaseOutputBudgets, reserveBudgets, settleBudgets, tokenReservation } from "./budgets.mjs";
import { boundedReceipt, discardReceiptBodies, outputReservation } from "./receipts.mjs";

const expiration = `o.status IN ('prepared','dispatched') AND (o.deadline_ms<=${clock}
OR (o.status='dispatched' AND EXISTS(SELECT 1 FROM nodes n WHERE n.id=o.claimant AND (n.lease<=${clock} OR n.draining<>0)))
OR (o.status='prepared' AND (s.lifecycle<>'open'
OR EXISTS(SELECT 1 FROM runs r WHERE r.id=o.run_id AND r.status IN ('completed','failed','cancelled','blocked'))
OR EXISTS(SELECT 1 FROM connections c WHERE c.id=o.recipient AND c.status<>'active')
OR EXISTS(SELECT 1 FROM interactions i WHERE i.session_id=o.session_id AND i.subject=o.id
AND (i.status IN ('rejected','cancelled','expired') OR i.expires_ms<=${clock})))))`;

export async function settlePackage(m, current, prior, receipt, receiptText) {
  if (prior.action !== "build_package") return;
  const config = decode(current.config);
  const action = (config.actions ?? []).find(value => value.name === prior.action);
  const template = (config.connections ?? []).find(value => value.name === action?.connection);
  if (!["local_compute", "modal", "fly"].includes(template?.provider)) return;
  const packageID = decode(prior.arguments).package_id;
  if (!packageID) return;
  await m.scope("package:" + current.tenant + ":" + packageID);
  const pkg = decode((await m.one("SELECT body FROM packages WHERE tenant=? AND id=?", [current.tenant, packageID])).body);
  if (pkg.status === "ready") return;
  pkg.status = receipt.outcome;
  pkg.error = receipt.error;
  if (receipt.outcome === "completed") {
    const value = decode(field(receiptText, "result") ?? "null");
    const immutable = /^(?:[A-Za-z0-9._:/-]+@)?sha256:[a-f0-9]{64}$/u;
    if (!value || value.package_id !== pkg.id || value.source_digest !== pkg.digest || value.builder !== template.provider ||
        !immutable.test(value.image) || !immutable.test(value.base_image)) {
      pkg.status = "failed";
      pkg.error = "completed build has invalid immutable image provenance";
    } else {
      pkg.status = "ready";
      pkg.image = value.image;
      delete pkg.error;
    }
  }
  m.change("UPDATE packages SET body=? WHERE tenant=? AND id=?", [encode(pkg), current.tenant, packageID]);
}

export async function discardOperation(m, prior) {
  if (prior.discarded) return;
  const receipt = await discardReceiptBodies(prior.usage ? { usage: decode(prior.usage) } : {});
  const usage = receipt.usage ? wire("Usage", receipt.usage) : "";
  m.change("UPDATE operations SET discarded=1,result_body='',usage=? WHERE session_id=? AND id=? AND discarded=0",
    [usage, prior.session_id, prior.id], true);
  if (big(prior.result_sequence) === 0n && ["completed", "failed"].includes(prior.status)) {
    await releaseOutputBudgets(m, prior);
  }
  prior.discarded = 1;
  prior.result_body = "";
  prior.usage = usage;
}

export const effectHandlers = {
  async Operation(db, [identifier, operationID]) {
    return operation(await one(db, "SELECT " + operationColumns + " FROM operations WHERE session_id=? AND id=?", [identifier, operationID]));
  },
  async Operations(db, [identifier, limit, after]) {
    return (await pageRows(db, operationColumns, "operations", "session_id=? AND id>?", "id", operationBytes, limit, [identifier, after])).map(operation);
  },
  async ReceiptOutputSchema(db, [identifier, operationID]) {
    const value = await one(db, `SELECT s.config,o.kind,o.action FROM operations o JOIN sessions s ON s.id=o.session_id
WHERE o.session_id=? AND o.id=? AND o.claimant<>''`, [identifier, operationID]);
    if (value.kind !== "action") return "";
    const action = (decode(value.config).actions ?? []).find(action => action.name === value.action);
    need(action, "not_found", "original receipt action is unavailable");
    return action.output_schema === undefined ? "" : encode(action.output_schema);
  },
  async DiscardDeletedResults(db, [limit]) {
    safe(limit, 1, 1024);
    const m = new Mutation(db);
    const candidate = await m.one("SELECT id FROM sessions WHERE cleanup_ready=1 ORDER BY cleanup_ms,id LIMIT 1", [], true);
    if (!candidate) return 0;
    const current = await m.session(candidate.id);
    need(current.lifecycle === "deleted", "corrupt", "result disposal requires a deleted session");
    const values = await m.rows(`SELECT id,session_id,status,CAST(result_sequence AS TEXT) AS result_sequence,output_limit,usage,discarded
FROM operations WHERE session_id=? AND discarded=0 ORDER BY id LIMIT 1`, [candidate.id]);
    for (const value of values) await discardOperation(m, value);
    m.change(`UPDATE sessions SET cleanup_ready=CASE WHEN
EXISTS(SELECT 1 FROM operations WHERE session_id=? AND discarded=0) THEN 1 ELSE 0 END,cleanup_ms=${clock} WHERE id=?`, [candidate.id, candidate.id]);
    await m.commit();
    return values.length;
  },
  async Prepare(db, [owner, request], text) {
    const m = new Mutation(db);
    const current = await m.owner(request.Session, owner, true);
    const argumentsText = field(text[1], "Arguments");
    const hash = await digest(argumentsText);
    const prior = await m.one("SELECT " + operationColumns + " FROM operations WHERE session_id=? AND id=?", [request.Session, request.ID], true);
    if (prior) {
      need(prior.arguments_digest === hash && prior.action === request.Action && prior.step_key === request.StepKey,
        "conflict", "harness step diverged from its durable operation");
      return operation(prior);
    }
    const active = await m.one("SELECT COUNT(*) AS count FROM runs WHERE id=? AND session_id=? AND status IN ('queued','active','waiting')", [request.Run, request.Session]);
    need(active.count === 1, "conflict", "run no longer admits operations");
    const now = (await m.one(`SELECT ${clock} AS now`)).now;
    const limits = decode(current.config).limits;
    need(limits.deadline_ms === undefined || now < safe(limits.deadline_ms), "forbidden", "session deadline reached");
    const intent = "EXISTS(SELECT 1 FROM allocations WHERE session_id=? AND sequence=? AND status='written' AND digest=?)";
    const recorded = await m.one("SELECT CASE WHEN " + intent + " THEN 1 ELSE 0 END AS valid", [request.Session, request.IntentSequence, request.IntentDigest]);
    need(recorded.valid === 1, "conflict", "dispatch intent has not durably reached the transcript");
    m.guard(intent, [request.Session, request.IntentSequence, request.IntentDigest]);
    const reserve = outputReservation(safe(request.OutputLimit, 1, 196608));
    const tokens = request.Kind === "model" ? tokenReservation(current.config, argumentsText) : 0;
    await reserveBudgets(m, request.Session, reserve, tokens);
    const deadline = Math.min(now + safe(request.Timeout, 1, 86400000), limits.deadline_ms === undefined ? Number.MAX_SAFE_INTEGER : safe(limits.deadline_ms));
    m.change(`INSERT INTO operations(id,session_id,run_id,step_key,kind,action,arguments,arguments_digest,recipient,output_limit,deadline_ms,
generation,approval,retry_of,intent_sequence,intent_digest,token_reserve) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)`,
    [request.ID, request.Session, request.Run, request.StepKey, request.Kind, request.Action, argumentsText, hash, request.Recipient,
      request.OutputLimit, deadline, request.Generation, request.Approval, request.RetryOf, request.IntentSequence, request.IntentDigest, tokens]);
    await journal(m, current, "prepared:" + request.ID, "operation.prepared", { id: request.ID, recipient: request.Recipient });
    return result(m, "SELECT " + operationColumns + " FROM operations WHERE id=?", [request.ID], operation);
  },
  async ClaimDispatch(db, [identifier, owner, operationID, claimant, requestKey]) {
    const m = new Mutation(db);
    const current = await m.owner(identifier, owner, true);
    const deadline = decode(current.config).limits.deadline_ms;
    if (deadline !== undefined) {
      const now = (await m.one(`SELECT ${clock} AS now`)).now;
      need(now < safe(deadline), "forbidden", "session deadline reached before dispatch");
      m.guard(`${clock}<?`, [deadline]);
    }
    const prior = await m.one("SELECT " + operationColumns + " FROM operations WHERE session_id=? AND id=?", [identifier, operationID]);
    need(prior.status === "prepared", "conflict", "operation was already claimed; replay is not execution permission");
    if (prior.approval !== 0) {
      const predicate = `(SELECT COUNT(*) FROM interactions WHERE session_id=? AND subject=? AND subject_digest=?
AND status='answered' AND expires_ms>${clock})=1`;
      const values = [identifier, operationID, prior.arguments_digest];
      need((await m.one("SELECT CASE WHEN " + predicate + " THEN 1 ELSE 0 END AS valid", values)).valid === 1,
        "forbidden", "operation has no current approval for these exact arguments");
      m.guard(predicate, values);
    }
    if (prior.recipient && prior.recipient !== "model" && prior.recipient !== "internal") {
      const values = [identifier, prior.recipient, prior.generation];
      const predicate = "EXISTS(SELECT 1 FROM connections WHERE session_id=? AND id=? AND status='active' AND generation=?)";
      need((await m.one("SELECT CASE WHEN " + predicate + " THEN 1 ELSE 0 END AS valid", values)).valid === 1 &&
        (claimant === owner.Node || claimant === prior.recipient), "forbidden", "connection is revoked, replaced or not the invocation recipient");
      m.guard(predicate, values);
      if (m.authority && claimant !== owner.Node) {
        const subject = await authoritySubject(m);
        const attached = await m.one("SELECT claim_subject FROM connections WHERE id=? AND session_id=?", [prior.recipient, identifier]);
        need(subject === prior.recipient || (attached.claim_subject && attached.claim_subject === subject),
          "forbidden", "claim is not from the attached application subject");
      }
    }
    m.change(`UPDATE operations SET status='dispatched',claimant=?,claim_key=?,revision=revision+1
WHERE id=? AND session_id=? AND status='prepared' AND deadline_ms>${clock}
AND EXISTS(SELECT 1 FROM runs r WHERE r.id=operations.run_id AND r.status IN ('queued','active','waiting'))`,
    [claimant, requestKey, operationID, identifier], true);
    return result(m, "SELECT " + operationColumns + " FROM operations WHERE id=?", [operationID], operation);
  },
  async RecordReceipt(db, [identifier, operationID, receipt, sequence, resultDigest], text) {
    const m = new Mutation(db);
    const current = await m.session(identifier);
    const discarded = current.lifecycle === "deleted";
    const prior = await m.one("SELECT " + operationColumns + " FROM operations WHERE session_id=? AND id=?", [identifier, operationID]);
    let body = text[2];
    const resultText = field(body, "result");
    let usage = field(body, "usage");
    const receiptHash = await digest(encode({
      Outcome: receipt.outcome, Result: raw(resultText), Usage: raw(usage), Error: raw(field(body, "error")),
    }));
    need(!prior.receipt_key || prior.receipt_key !== receipt.request_id || prior.receipt_hash === receiptHash,
      "conflict", "receipt request key has different evidence");
    if ((prior.status === "unknown" && receipt.outcome === "unknown" && prior.receipt_hash === receiptHash) ||
        ["completed", "failed"].includes(prior.status)) {
      need(prior.receipt_hash === receiptHash, "conflict", "conflicting settled receipt");
      if (discarded) {
        await discardOperation(m, prior);
        return result(m, "SELECT " + operationColumns + " FROM operations WHERE id=?", [operationID], operation);
      }
      return operation(prior);
    }
    need(["dispatched", "unknown"].includes(prior.status), "conflict", "receipt has no committed dispatch claim");
    need(["completed", "failed", "unknown"].includes(receipt.outcome), "invalid", "invalid receipt outcome");
    const normalized = await boundedReceipt(receipt, resultText, usage ? field(usage, "provider") : undefined, prior.output_limit);
    receipt = normalized.receipt;
    body = normalized.body;
    usage = normalized.usage;
    const error = receipt.error ?? "";
    if (normalized.unavailable) {
      body = "";
      sequence = 0;
      resultDigest = "";
    }
    if (receipt.outcome === "completed" && receipt.result === undefined && receipt.error !== undefined) body = "";
    if (discarded) {
      body = "";
      sequence = 0;
      resultDigest = "";
      receipt = await discardReceiptBodies(receipt);
      usage = receipt.usage ? wire("Usage", receipt.usage) : "";
    }
    need(big(sequence) >= 0n && (big(sequence) !== 0n || resultDigest === ""), "invalid", "receipt result position is invalid");
    if (big(sequence) > 0n) {
      m.guard("EXISTS(SELECT 1 FROM allocations WHERE session_id=? AND sequence=? AND status='written' AND digest=?)", [identifier, sequence, resultDigest]);
    }
    const counters = receipt.usage ? wire("UsageCounters", receipt.usage) : "";
    m.change(`UPDATE operations SET status=?,revision=revision+1,result_sequence=?,result_digest=?,result_body=?,usage=?,receipt_counters=?,error=?,receipt_hash=?,receipt_key=?,discarded=?
WHERE id=? AND session_id=? AND status IN ('dispatched','unknown')`,
    [receipt.outcome, sequence, resultDigest, body, usage ?? "", counters, error, receiptHash, receipt.request_id, discarded ? 1 : 0, operationID, identifier], true);
    if (!discarded) await settlePackage(m, current, prior, receipt, body || wire("Receipt", receipt));
    if (receipt.outcome !== "unknown") {
      await settleBudgets(m, prior, receipt, new TextEncoder().encode(body).length, false, discarded || big(sequence) > 0n);
    }
    m.change("UPDATE sessions SET ready=CASE WHEN lifecycle='deleted' THEN 0 ELSE 1 END WHERE id=?", [identifier]);
    return result(m, "SELECT " + operationColumns + " FROM operations WHERE id=?", [operationID], operation);
  },
  async BindResult(db, [identifier, owner, operationID, sequence, hash]) {
    const m = new Mutation(db);
    await m.owner(identifier, owner);
    const prior = await m.one("SELECT " + operationColumns + " FROM operations WHERE session_id=? AND id=?", [identifier, operationID]);
    if (big(sequence) > 0n && prior.result_sequence === String(sequence) && prior.result_digest === hash) return;
    m.change(`UPDATE operations SET result_sequence=?,result_digest=? WHERE id=? AND session_id=? AND result_sequence=0
AND status IN ('completed','failed','unknown') AND EXISTS(SELECT 1 FROM allocations a WHERE a.session_id=operations.session_id
AND a.sequence=? AND a.status='written' AND a.digest=?)`, [sequence, hash, operationID, identifier, sequence, hash], true);
    if (["completed", "failed"].includes(prior.status)) await releaseOutputBudgets(m, prior);
    await m.commit();
  },
  async ExpireOperations(db, [limit]) {
    const m = new Mutation(db);
    const candidates = await m.rows(`SELECT o.session_id,o.id FROM operations o JOIN sessions s ON s.id=o.session_id
WHERE ${expiration} ORDER BY o.deadline_ms,o.id LIMIT ?`, [Math.min(safe(limit, 1, 1024), 1)]);
    let count = 0;
    for (const value of candidates) {
      const current = await m.session(value.session_id);
      const prior = await m.one("SELECT " + operationColumns + " FROM operations WHERE session_id=? AND id=?", [value.session_id, value.id]);
      if (!["prepared", "dispatched"].includes(prior.status)) continue;
      const predicate = `EXISTS(SELECT 1 FROM operations o JOIN sessions s ON s.id=o.session_id WHERE o.id=? AND ${expiration})`;
      if ((await m.one("SELECT CASE WHEN " + predicate + " THEN 1 ELSE 0 END AS valid", [prior.id])).valid !== 1) continue;
      m.guard(predicate, [prior.id]);
      const prepared = prior.status === "prepared";
      const discarded = current.lifecycle === "deleted";
      const status = prepared ? "failed" : "unknown";
      const reason = prepared ? "operation was never dispatched before expiry, cancellation, revocation or rejected approval"
        : "dispatch deadline or owner was lost; the original recipient may still supply authenticated evidence";
      const body = prepared && !discarded ? wire("Receipt", { request_id: "not-dispatched:" + prior.id, outcome: "failed", error: reason }) : "";
      if (prepared) {
        await settleBudgets(m, prior, {}, new TextEncoder().encode(body).length, true, discarded);
      }
      m.change("UPDATE operations SET status=?,result_body=?,error=?,discarded=?,revision=revision+1 WHERE id=? AND status=?",
        [status, body, reason, discarded ? 1 : 0, prior.id, prior.status], true);
      m.change("UPDATE sessions SET ready=CASE WHEN lifecycle='deleted' THEN 0 ELSE 1 END WHERE id=?", [value.session_id]);
      count++;
    }
    const interactions = await m.rows(`SELECT id,session_id FROM interactions WHERE status='pending' AND expires_ms<=${clock} ORDER BY expires_ms,id LIMIT 2`);
    for (const interaction of interactions) {
      await m.session(interaction.session_id);
      m.change(`UPDATE interactions SET status='expired',revision=revision+1 WHERE id=? AND status='pending' AND expires_ms<=${clock}`, [interaction.id]);
      m.change("UPDATE sessions SET ready=1 WHERE id=?", [interaction.session_id]);
    }
    await m.commit();
    return count;
  },
};
