import { Mutation, big, maximumInteger, need, safe } from "./domain.mjs";
import { wakeParents } from "./children.mjs";

export async function pauseLineage(m, identifier, paused) {
  await m.scope("budget:" + identifier);
  m.change("UPDATE budgets SET paused=? WHERE id=?", [paused, identifier], true);
}

export async function cancelLineage(m, identifier) {
  await m.scope("budget:" + identifier);
  const before = await m.one("SELECT CAST(cancel_generation AS TEXT) AS generation FROM budgets WHERE id=?", [identifier]);
  const generation = big(before.generation) + 1n;
  need(generation <= maximumInteger, "overloaded", "cancellation generation exhausted");
  m.change("UPDATE budgets SET cancel_generation=? WHERE id=? AND cancel_generation=?", [generation, identifier, before.generation], true);
  await m.scope("cascade:" + identifier);
  m.change(`INSERT INTO control_cascades(session_id,generation) VALUES(?,?)
ON CONFLICT(session_id) DO UPDATE SET generation=excluded.generation,after_id='',status='pending'`, [identifier, generation]);
}

export const lineageHandlers = {
  async PropagateControls(db, [limit]) {
    const m = new Mutation(db);
    const cascade = await m.one(`SELECT session_id,CAST(generation AS TEXT) AS generation,after_id
FROM control_cascades WHERE status='pending' ORDER BY session_id LIMIT 1`, [], true);
    if (!cascade) return [];
    await m.scope("cascade:" + cascade.session_id);
    const candidates = await m.rows(`SELECT session_id FROM budget_members
WHERE budget_id=? AND session_id<>? AND generation<? AND session_id>? ORDER BY session_id LIMIT ?`,
    [cascade.session_id, cascade.session_id, cascade.generation, cascade.after_id, Math.min(safe(limit, 1, 128), 4)]);
    const closed = [];
    for (const { session_id: identifier } of candidates) {
      const current = await m.session(identifier);
      if (current.lifecycle === "deleted") continue;
      for (const query of [
        "UPDATE sessions SET lifecycle='closed',ready=1,revision=revision+1 WHERE id=? AND lifecycle<>'closed'",
        "UPDATE runs SET status='cancelled' WHERE session_id=? AND status IN ('queued','active','waiting','paused','blocked')",
        "UPDATE commands SET status='cancelled' WHERE session_id=? AND status='accepted' AND kind IN ('message','action','event')",
        "UPDATE interactions SET status='cancelled',revision=revision+1 WHERE session_id=? AND status='pending'",
        "UPDATE schedules SET status='cancelled' WHERE session_id=? AND status='active'",
        wakeParents,
      ]) m.change(query, [identifier]);
      closed.push(identifier);
    }
    const after = candidates.length ? candidates[candidates.length - 1].session_id : cascade.after_id;
    m.change(`UPDATE control_cascades SET after_id=?,status=? WHERE session_id=? AND generation=? AND after_id=? AND status='pending'`,
      [after, candidates.length ? "pending" : "done", cascade.session_id, cascade.generation, cascade.after_id], true);
    await m.commit();
    return closed;
  },
};
