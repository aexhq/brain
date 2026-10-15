import {
  Mutation, big, clock, decode, digest, encode, id, identifiers, need, operation, operationColumns, raw, result, safe, wire,
} from "./domain.mjs";
import { refineTokenBudgets } from "./budgets.mjs";
import { discardOperation } from "./effects.mjs";
import { bytes, pageRows } from "./pages.mjs";

const standard = ["input_tokens", "output_tokens"];
function reported(usage) {
  if (standard.some(key => big(usage[key] ?? 0) > (1n << 40n))) return [0n, false];
  return [standard.reduce((sum, key) => sum + big(usage[key] ?? 0), 0n), standard.every(key => usage[key] !== undefined)];
}

export const usageHandlers = {
  async ReportUsage(db, [identifier, operationID, request, reporter]) {
    identifiers([request.request_id, reporter.Subject]);
    const m = new Mutation(db);
    const current = await m.session(identifier);
    const prior = await m.one("SELECT " + operationColumns + " FROM operations WHERE session_id=? AND id=?", [identifier, operationID]);
    if (reporter.Kind === "operator") need(reporter.Tenant === current.tenant, "not_found", "session not found");
    else need(reporter.Kind === "receipt" && prior.claimant && reporter.Subject === prior.claimant && reporter.Tenant === "",
      "forbidden", "usage evidence requires an original recipient or operator attestation");
    need(prior.kind === "model" && prior.claimant && ["completed", "failed"].includes(prior.status),
      "conflict", "only a settled originally dispatched model accepts supplemental usage");
    const counterText = wire("UsageCounters", request.counters);
    const requestText = wire("UsageSupplement", { ...request, counters: raw(counterText) });
    const hash = await digest(encode([raw(requestText), reporter.Kind, reporter.Subject, reporter.Tenant]));
    const existing = await m.one("SELECT request_hash FROM usage_evidence WHERE operation_id=? AND request_key=?",
      [operationID, request.request_id], true);
    need(!existing || existing.request_hash === hash, "conflict", "usage request key has different evidence or attribution");
    if (current.lifecycle === "deleted") await discardOperation(m, prior);
    if (existing) return result(m, "SELECT " + operationColumns + " FROM operations WHERE id=?", [operationID], operation);
    const usage = prior.usage ? decode(prior.usage) : { provider: {} };
    const [before, wasComplete] = reported(usage);
    let changed = false;
    for (const key of [...standard, "cached_tokens"]) {
      if (request.counters[key] === undefined) continue;
      const value = safe(request.counters[key], 0, 2 ** 40);
      if (usage[key] !== undefined) {
        need(big(usage[key]) === BigInt(value), "conflict", "supplemental evidence cannot replace a reported counter");
        continue;
      }
      usage[key] = value;
      changed = true;
    }
    need(changed, "conflict", "report must fill an absent counter; reconcile the original request_id instead");
    const [after, complete] = reported(usage);
    need(after >= before, "corrupt", "usage refinement cannot erase reported accounting");
    const count = (await m.one("SELECT COUNT(*) AS count FROM usage_evidence WHERE operation_id=?", [operationID])).count;
    need(count < 3, "corrupt", "usage evidence exceeds the three-counter refinement bound");
    const evidence = {
      id: await id("usage", identifier, operationID, request.request_id),
      session_id: identifier, operation_id: operationID, request_id: request.request_id,
      source: reporter.Kind, subject: reporter.Subject, counters: raw(counterText), accepted_ms: (await m.one(`SELECT ${clock} AS now`)).now,
    };
    m.change("INSERT INTO usage_evidence(id,session_id,operation_id,request_key,request_hash,body) VALUES(?,?,?,?,?,?)",
      [evidence.id, identifier, operationID, request.request_id, hash, wire("UsageEvidence", evidence)]);
    m.change("UPDATE operations SET usage=?,revision=revision+1 WHERE session_id=? AND id=? AND status IN ('completed','failed')",
      [wire("Usage", usage), identifier, operationID], true);
    await refineTokenBudgets(m, prior, after - before, !wasComplete && complete);
    return result(m, "SELECT " + operationColumns + " FROM operations WHERE id=?", [operationID], operation);
  },
  async UsageEvidence(db, [identifier, operationID, limit, after]) {
    let predicate = "session_id=? AND id>?";
    const values = [identifier, after];
    if (operationID !== "") { predicate += " AND operation_id=?"; values.push(operationID); }
    return (await pageRows(db, "body", "usage_evidence", predicate, "id", bytes("body"), limit, values)).map(value => decode(value.body));
  },
};
