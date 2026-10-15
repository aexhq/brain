import { need } from "./domain.mjs";

export async function documentQuota(mutation, session, scope, key, value) {
  const size = new TextEncoder().encode(value).byteLength;
  need(size <= 65536, "invalid", "document exceeds its byte bound");
  const row = await mutation.one(`SELECT COUNT(*) AS count,COALESCE(SUM(LENGTH(CAST(value AS BLOB))),0) AS size,
COALESCE(MAX(CASE WHEN scope=? AND key=? THEN LENGTH(CAST(value AS BLOB)) ELSE 0 END),0) AS previous,
COALESCE(MAX(CASE WHEN scope=? AND key=? THEN 1 ELSE 0 END),0) AS present FROM documents WHERE session_id=?`,
  [scope, key, scope, key, session]);
  need(row.size - row.previous + size <= 4 * 1024 * 1024 && (row.present || row.count < 1024),
    "overloaded", "state and memory quota reached");
}

export function bounded(text, size, message) {
  need(new TextEncoder().encode(text).byteLength <= size, "invalid", message);
}
