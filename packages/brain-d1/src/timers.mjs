import {
  Fault, Mutation, clock, decode, digest, field, id, invalidParticipantAncestor, need, one, result, rows, safe, schedule, scheduleColumns,
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
    const authority = db.authority, parent = authority?.Participant ?? "";
    if (authority) {
      need(authority.Tenant === current.tenant && (!authority.Session || authority.Session === identifier),
        "forbidden", "participant grant exceeds its tenant or session authority");
      let subject = authority.Subject, permissions = authority.Permissions, expires = authority.Expires;
      if (parent) {
        const currentParent = await m.one("SELECT subject,permissions,expires_ms FROM participants WHERE id=? AND session_id=?", [parent, identifier]);
        subject = currentParent.subject;
        permissions = decode(currentParent.permissions);
        expires = currentParent.expires_ms;
      }
      need(request.subject === subject && (permissions.includes("*") || request.permissions.every(value => permissions.includes(value))) &&
        safe(request.expires_ms) <= expires, "forbidden", "participant cannot amplify permissions, lifetime or source identity");
    }
    const prior = await m.one("SELECT request_hash,parent,revoked FROM participants WHERE id=?", [participantID], true);
    if (prior) need(prior.request_hash === hash && prior.parent === parent, "conflict", "participant key has different arguments or parent authority");
    else {
      const count = await m.one("SELECT COUNT(*) AS count FROM participants WHERE session_id=?", [identifier]);
      const depth = await m.one("SELECT COALESCE(MAX(depth),0) AS depth FROM participant_ancestors WHERE descendant=?", [parent]);
      need(count.count < 1024 && depth.depth < 8, "overloaded", "participant count or delegation depth bound reached");
      m.change("INSERT INTO participants(id,session_id,subject,permissions,expires_ms,request_hash,parent) VALUES(?,?,?,?,?,?,?)",
        [participantID, identifier, request.subject, field(text[1], "permissions"), request.expires_ms, hash, parent]);
      if (parent) {
        m.change("INSERT INTO participant_ancestors(descendant,ancestor,depth) VALUES(?,?,1)", [participantID, parent]);
        m.change("INSERT INTO participant_ancestors(descendant,ancestor,depth) SELECT ?,ancestor,depth+1 FROM participant_ancestors WHERE descendant=?",
          [participantID, parent]);
      }
    }
    await m.commit();
    return { ID: participantID, Parent: parent, Session: identifier, Tenant: current.tenant, Subject: request.subject,
      Permissions: request.permissions, Expires: request.expires_ms, Revoked: Boolean(prior?.revoked) };
  },
  async Participant(db, [participantID]) {
    const value = await one(db, `SELECT p.id,p.parent,p.session_id,s.tenant,p.subject,p.permissions,p.expires_ms,
CASE WHEN p.revoked<>0 OR EXISTS(${invalidParticipantAncestor()}) THEN 1 ELSE 0 END AS revoked
FROM participants p JOIN sessions s ON p.session_id=s.id WHERE p.id=? AND s.lifecycle<>'deleted' AND p.expires_ms>${clock}`, [participantID]);
    return { ID: value.id, Parent: value.parent, Session: value.session_id, Tenant: value.tenant, Subject: value.subject,
      Permissions: decode(value.permissions), Expires: value.expires_ms, Revoked: value.revoked !== 0 };
  },
  async RevokeParticipant(db, [identifier, participantID]) {
    const m = new Mutation(db);
    await m.session(identifier);
    if (db.authority?.Participant && db.authority.Participant !== participantID) {
      const allowed = await m.one("SELECT COUNT(*) AS count FROM participant_ancestors WHERE descendant=? AND ancestor=?", [participantID, db.authority.Participant]);
      need(allowed.count === 1, "forbidden", "participants may revoke only themselves or their descendants");
    }
    m.change("UPDATE participants SET revoked=1 WHERE session_id=? AND id=?", [identifier, participantID], true);
    await m.commit();
  },
};
