import { connection, connectionColumns, fileColumns, interaction, interactionColumns, one, operation, operationColumns, rows, run, runColumns, safe } from "./domain.mjs";
import { childrenReady } from "./children.mjs";

export const bytes = column => "LENGTH(CAST(" + column + " AS BLOB))";
export const runBytes = [ "r.checkpoint", "r.decision", "r.error", "c.payload" ].map(bytes).join("+");
export const operationBytes = ["arguments", "result_body", "usage"].map(bytes).join("+") + "+6*" + bytes("error");

export async function pageRows(db, columns, source, predicate, key, size, limit, values) {
  const result = await rows(db, `WITH candidates AS (
SELECT ${columns},${key} AS page_key,${size}+4096 AS body_bytes FROM ${source} WHERE ${predicate} ORDER BY ${key} LIMIT ?),
bounded AS (SELECT candidates.*,SUM(body_bytes) OVER(ORDER BY page_key ROWS UNBOUNDED PRECEDING) AS page_bytes FROM candidates)
SELECT * FROM bounded WHERE page_bytes<=? ORDER BY page_key`, [...values, safe(limit, 1, 1024), 2 * 1024 * 1024]);
  return result.map(({ page_key, page_bytes, body_bytes, ...value }) => value);
}

export const runnable = `r.status IN ('queued','active') OR (r.status='waiting' AND (
(r.wait_kind='operation' AND EXISTS(SELECT 1 FROM operations o WHERE o.id=r.wait_id AND o.status IN ('completed','failed','unknown')))
OR (r.wait_kind='dispatch' AND EXISTS(SELECT 1 FROM operations o WHERE o.id=r.wait_id AND
(o.status<>'prepared' OR o.approval=0 OR EXISTS(SELECT 1 FROM interactions i
WHERE i.session_id=o.session_id AND i.subject=o.id AND i.subject_digest=o.arguments_digest AND i.status='answered'))))
OR (r.wait_kind='interaction' AND EXISTS(SELECT 1 FROM interactions i WHERE i.id=r.wait_id AND i.status<>'pending'))
OR (r.wait_kind='child' AND NOT EXISTS(SELECT 1 FROM runs child WHERE child.session_id=r.wait_id AND child.status IN ('queued','active','waiting','paused'))
AND NOT EXISTS(SELECT 1 FROM commands child WHERE child.session_id=r.wait_id AND child.status='accepted' AND child.kind IN ('message','action','event')))
OR (${childrenReady})
))`;

export const pageHandlers = {
  async RunnableRuns(db, [identifier, limit]) {
    return (await pageRows(db, runColumns, "runs r JOIN commands c ON c.id=r.command_id",
      "r.session_id=? AND (" + runnable + ")", "r.id", runBytes, limit, [identifier])).map(run);
  },
  async PendingResults(db, [identifier, limit]) {
    return (await pageRows(db, operationColumns, "operations", "session_id=? AND result_sequence=0 AND status IN ('completed','failed','unknown')",
      "id", operationBytes, limit, [identifier])).map(operation);
  },
  async ChildState(db, [identifier]) {
    const value = await one(db, `SELECT
(SELECT COUNT(*) FROM runs WHERE session_id=? AND status IN ('queued','active','waiting','paused'))+
(SELECT COUNT(*) FROM commands WHERE session_id=? AND status='accepted' AND kind IN ('message','action','event')) AS pending,
(SELECT COUNT(*) FROM runs WHERE session_id=? AND status IN ('failed','cancelled','blocked'))+
(SELECT COUNT(*) FROM commands WHERE session_id=? AND status IN ('failed','cancelled') AND kind IN ('message','action','event') AND run_id='') AS failed,
(SELECT COUNT(*) FROM runs WHERE session_id=? AND status='completed') AS completed`, [identifier, identifier, identifier, identifier, identifier]);
    return { Pending: value.pending, Failed: value.failed, Completed: value.completed };
  },
  async Interaction(db, [identifier, interactionID]) {
    return interaction(await one(db, "SELECT " + interactionColumns + " FROM interactions WHERE session_id=? AND id=?", [identifier, interactionID]));
  },
  async ActiveConnection(db, [identifier, template]) {
    return connection(await one(db, "SELECT " + connectionColumns + " FROM connections WHERE session_id=? AND template=? AND status='active' ORDER BY generation DESC,id LIMIT 1", [identifier, template]));
  },
  async File(db, [identifier, name]) {
    return one(db, "SELECT " + fileColumns + " FROM files WHERE session_id=? AND name=?", [identifier, name]);
  },
};
