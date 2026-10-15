import { Mutation, big, clock, decode, digest, field, need, one, rows, safe } from "./domain.mjs";
import { equal } from "./budgets.mjs";

export const wakeParents = `WITH changed(id) AS (VALUES(?))
UPDATE sessions SET ready=1 WHERE id IN (
SELECT c.session_id FROM run_children c JOIN changed ON c.child_id=changed.id
UNION SELECT r.session_id FROM runs r JOIN changed ON r.wait_id=changed.id WHERE r.status='waiting' AND r.wait_kind='child')`;

export const childrenReady = `r.wait_kind='children' AND NOT EXISTS(
SELECT 1 FROM run_children links JOIN commands input ON input.id=links.command_id
LEFT JOIN runs child ON child.id=input.run_id
WHERE links.session_id=r.session_id AND links.run_id=r.id AND links.step=r.step-1
AND (input.status='accepted' OR child.status IN ('queued','active','waiting','paused')))`;

export function delegations(decision) {
  const children = decision?.kind === "delegate" ? [decision.delegation] : decision?.kind === "fork" ? decision.delegations : [];
  need(Array.isArray(children) && children.length > 0 && children.length <= 8 && children.every(Boolean),
    "invalid", "a delegation decision requires one to eight children");
  return children;
}

export async function admitChild(m, parent, origin, request) {
  const { owner, runID, step } = origin;
  const index = safe(origin.index, 0, 7);
  await m.owner(parent, owner, true);
  const current = await m.one("SELECT decision FROM runs WHERE session_id=? AND id=? AND step=? AND status='active' AND decision<>''",
    [parent, runID, step], true);
  need(current, "conflict", "delegating run no longer holds this decision boundary");
  const children = delegations(decode(current.decision));
  const child = children[index];
  need(child && typeof request.message === "string" && equal(child.config, request.config) &&
    ((child.context ?? []).length ? request.message.startsWith(child.message + "\nUntrusted shared observation:\n") : request.message === child.message),
  "conflict", "child differs from its saved decision");
  m.guard("EXISTS(SELECT 1 FROM runs WHERE session_id=? AND id=? AND step=? AND status='active' AND decision=?)",
    [parent, runID, step, current.decision]);
}

export async function guardChildWait(m, identifier, runID, step, decision, wait) {
  if (wait?.kind !== "children") return;
  const children = delegations(decode(decision));
  need(wait.id === String(step), "conflict", "child wait differs from its saved decision");
  const predicate = "(SELECT COUNT(*) FROM run_children WHERE session_id=? AND run_id=? AND step=?)=?";
  const values = [identifier, runID, step, children.length];
  const complete = await m.one("SELECT CASE WHEN " + predicate + " THEN 1 ELSE 0 END AS valid", values);
  need(complete.valid === 1, "conflict", "not every saved child has been admitted");
  m.guard(predicate, values);
}

export const childHandlers = {
  async Children(db, [identifier, runID, step]) {
    safe(step, 0, 4294967295);
    const group = await one(db, "SELECT expected FROM child_groups WHERE session_id=? AND run_id=? AND step=?", [identifier, runID, step]);
    const values = await rows(db, `SELECT links.ordinal,links.child_id,links.command_id,input.status AS input_status,input.run_id,
COALESCE(child.status,'') AS status,COALESCE(child.error,input.error) AS error,
CAST(COALESCE(child.completion_sequence,0) AS TEXT) AS sequence,COALESCE(child.completion_digest,'') AS digest
FROM run_children links JOIN commands input ON input.id=links.command_id
LEFT JOIN runs child ON child.id=input.run_id AND child.session_id=links.child_id
WHERE links.session_id=? AND links.run_id=? AND links.step=? ORDER BY links.ordinal LIMIT 9`, [identifier, runID, step]);
    need(values.length <= 8 && values.length <= group.expected, "corrupt", "delegation exceeds its bounded decision");
    const children = values.map(value => {
      const status = value.status || (value.input_status === "accepted" ? "queued" : value.input_status);
      need(value.status || ["accepted", "failed", "cancelled"].includes(value.input_status), "corrupt", "delegated input has no matching run");
      const result = { index: value.ordinal, session_id: value.child_id, command_id: value.command_id,
        run_id: value.run_id, status, availability: ["completed", "failed", "cancelled", "blocked"].includes(status) ? "none" : "pending" };
      if (status === "completed") {
        result.availability = "unavailable";
        if (big(value.sequence) > 0n && value.digest.length === 64) {
          result.availability = "referenced";
          result.completion = { session_id: value.child_id, sequence: value.sequence, digest: value.digest };
        }
      }
      if (value.error) result.error = decode(value.error);
      return result;
    });
    return { session_id: identifier, run_id: runID, step, expected: group.expected,
      ready: children.length === group.expected && children.every(value => value.availability !== "pending"), children };
  },
  async CompleteRun(db, [identifier, owner, runID, step, record], text) {
    safe(step, 0, 4294967294);
    need(typeof record.sequence === "string" && /^[1-9][0-9]*$/u.test(record.sequence) &&
      big(record.sequence) <= 9223372036854775807n && record.session_id === identifier && record.run_id === runID &&
      record.kind === "run.completed" && record.operation_id == null && record.data !== undefined,
    "invalid", "completion must name its original run record");
    const hash = await digest(text[4]);
    const m = new Mutation(db);
    await m.owner(identifier, owner, true);
    const current = await m.one(`SELECT decision,status,CAST(step AS TEXT) AS step,CAST(completion_sequence AS TEXT) AS sequence,completion_digest AS digest
FROM runs WHERE session_id=? AND id=?`, [identifier, runID]);
    if (current.status === "completed" && current.sequence === record.sequence && current.digest === hash) return;
    need(current.status === "active" && big(current.step) === big(step) && current.decision, "conflict", "run no longer holds this completion decision");
    const decision = decode(current.decision);
    need(decision.kind === "complete" && decision.result !== undefined && equal(record.data, { result: decision.result }),
      "conflict", "completion differs from its saved decision");
    m.change(`UPDATE runs SET status='completed',step=step+1,checkpoint=?,decision='',wait_kind='',wait_id='',error='',
completion_sequence=?,completion_digest=?,updated_ms=${clock}
WHERE id=? AND session_id=? AND status='active' AND step=? AND EXISTS(
SELECT 1 FROM allocations WHERE session_id=? AND sequence=? AND digest=? AND status='written')`,
    [field(current.decision, "checkpoint") ?? "null", record.sequence, hash, runID, identifier, step, identifier, record.sequence, hash], true);
    m.change(wakeParents, [identifier]);
    await m.commit();
  },
};
