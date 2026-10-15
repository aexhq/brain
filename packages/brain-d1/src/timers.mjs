import {
  Fault, Mutation, clock, decode, digest, field, id, need, one, result, rows, safe, schedule, scheduleColumns,
} from "./domain.mjs";
import { acceptInput } from "./control.mjs";
import { bytes, pageRows } from "./pages.mjs";
import { bounded } from "./quotas.mjs";

export const timerHandlers = {
  async Schedule(db, [identifier, request], text) {
    const timerID = await id("schedule", identifier, request.request_id);
    const hash = await digest(text[1]);
    const m = new Mutation(db);
    const current = await m.session(identifier, true);
    bounded(field(text[1], "input"), safe(decode(current.config).limits.input_bytes), "scheduled input exceeds its admitted bound");
    if (request.interval_ms !== undefined) safe(request.interval_ms, 1000);
    safe(request.due_ms);
    const previous = await m.one("SELECT request_hash FROM schedules WHERE id=?", [timerID], true);
    if (previous) need(previous.request_hash === hash, "conflict", "schedule key has different content");
    else {
      const active = await m.one("SELECT COUNT(*) AS count FROM schedules WHERE session_id=? AND status='active'", [identifier]);
      need(active.count < 128, "overloaded", "active schedule limit reached");
      m.change("INSERT INTO schedules(id,session_id,due_ms,interval_ms,request,request_hash) VALUES(?,?,?,?,?,?)",
        [timerID, identifier, request.due_ms, request.interval_ms ?? 0, text[1], hash]);
    }
    return result(m, "SELECT " + scheduleColumns + " FROM schedules WHERE id=?", [timerID], schedule);
  },
  async Schedules(db, [identifier, limit, after]) {
    return (await pageRows(db, scheduleColumns, "schedules", "session_id=? AND id>?", "id", bytes("request"), limit, [identifier, after])).map(schedule);
  },
  async CancelSchedule(db, [identifier, timerID]) {
    const m = new Mutation(db);
    await m.session(identifier);
    m.change("UPDATE schedules SET status='cancelled' WHERE session_id=? AND id=?", [identifier, timerID], true);
    return result(m, "SELECT " + scheduleColumns + " FROM schedules WHERE id=?", [timerID], schedule);
  },
  async FireTimers(db, [limit]) {
    const m = new Mutation(db);
    const now = (await m.one(`SELECT ${clock} AS now`)).now;
    const due = await m.rows("SELECT " + scheduleColumns + " FROM schedules WHERE status='active' AND due_ms<=? ORDER BY due_ms,id LIMIT ?",
      [now, Math.min(safe(limit, 1, 1024), 8)]);
    let count = 0;
    for (const timer of due) {
      const current = await m.session(timer.session_id);
      if (current.lifecycle !== "open") continue;
      const request = decode(timer.request);
      try {
        await acceptInput(m, { Tenant: current.tenant, Subject: "timer" }, current,
          { request_id: "timer:" + timer.id + ":" + timer.due_ms, data: request.input }, field(timer.request, "input"));
      } catch (error) {
        if (error instanceof Fault && error.code === "overloaded") continue;
        throw error;
      }
      const interval = request.interval_ms === undefined ? 0 : safe(request.interval_ms, 1000);
      if (interval === 0) {
        m.change("UPDATE schedules SET status='fired' WHERE id=? AND due_ms=? AND status='active'", [timer.id, timer.due_ms], true);
      } else {
        const next = timer.due_ms + (Math.floor((now - timer.due_ms) / interval) + 1) * interval;
        need(Number.isSafeInteger(next), "invalid", "next scheduled time exceeds the exact time domain");
        m.change("UPDATE schedules SET due_ms=? WHERE id=? AND due_ms=? AND status='active'", [next, timer.id, timer.due_ms], true);
      }
      count++;
    }
    await m.commit();
    return count;
  },
  async Grant(db, [identifier, request], text) {
    const subject = request.subject.toLowerCase();
    need(/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(request.subject) &&
      !["operator", "kernel", "local", "timer", "creator"].includes(subject) && !subject.startsWith("harness:") &&
      request.permissions.length <= 64 && request.permissions.every(value => value === "*" || /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(value)),
    "invalid", "participant subject and permissions must be bounded non-system identifiers");
    const participantID = await id("participant", identifier, request.request_id);
    const hash = await digest(text[1]);
    const m = new Mutation(db);
    const current = await m.session(identifier, true);
    const now = (await m.one(`SELECT ${clock} AS now`)).now;
    need(safe(request.expires_ms) > now && safe(request.expires_ms) <= now + 86400000, "invalid", "participant token expiry must be within one day");
    const prior = await m.one("SELECT request_hash FROM participants WHERE id=?", [participantID], true);
    if (prior) need(prior.request_hash === hash, "conflict", "participant key has different arguments");
    else m.change("INSERT INTO participants(id,session_id,subject,permissions,expires_ms,request_hash) VALUES(?,?,?,?,?,?)",
      [participantID, identifier, request.subject, field(text[1], "permissions"), request.expires_ms, hash]);
    await m.commit();
    return { ID: participantID, Session: identifier, Tenant: current.tenant, Subject: request.subject,
      Permissions: request.permissions, Expires: request.expires_ms, Revoked: false };
  },
  async Participant(db, [participantID]) {
    const value = await one(db, `SELECT p.id,p.session_id,s.tenant,p.subject,p.permissions,p.expires_ms,p.revoked
FROM participants p JOIN sessions s ON p.session_id=s.id WHERE p.id=? AND s.lifecycle<>'deleted' AND p.expires_ms>${clock}`, [participantID]);
    return { ID: value.id, Session: value.session_id, Tenant: value.tenant, Subject: value.subject,
      Permissions: decode(value.permissions), Expires: value.expires_ms, Revoked: value.revoked !== 0 };
  },
  async RevokeParticipant(db, [identifier, participantID]) {
    const m = new Mutation(db);
    await m.session(identifier);
    m.change("UPDATE participants SET revoked=1 WHERE session_id=? AND id=?", [identifier, participantID], true);
    await m.commit();
  },
};
