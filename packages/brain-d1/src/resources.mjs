import { cancelLineage } from "./lineage.mjs";
import { wakeParents } from "./children.mjs";
import { equal } from "./budgets.mjs";
import {
  Mutation, big, capability, clock, connection, connectionColumns, decode, digest, document, documentColumns, encode, field, fileColumns,
  id, interaction, interactionColumns, integer, need, one, operationColumns, raw, result, rows, safe, session, sessionColumns,
} from "./domain.mjs";
import { journal } from "./control.mjs";
import { bytes, pageRows } from "./pages.mjs";
import { bounded, documentQuota } from "./quotas.mjs";

export const resourceHandlers = {
  async Interact(db, [identifier, request], text) {
    bounded(text[1], 65536, "interaction exceeds its bounded fields");
    const interactionID = await id("interaction", identifier, request.request_id);
    const hash = await digest(text[1]);
    const m = new Mutation(db);
    await m.session(identifier, true);
    const prior = await m.one("SELECT request_hash FROM interactions WHERE id=?", [interactionID], true);
    if (prior) need(prior.request_hash === hash, "conflict", "interaction key has different content");
    else {
      const now = (await m.one(`SELECT ${clock} AS now`)).now;
      need(safe(request.expires_ms) > now && safe(request.expires_ms) <= now + 7 * 86400000, "invalid", "interaction expiry must be within seven days");
      if (request.kind === "approval") {
        const operation = await m.one("SELECT " + operationColumns + " FROM operations WHERE session_id=? AND id=?", [identifier, request.subject]);
        need(operation.status === "prepared" && operation.run_id === request.run_id && operation.arguments_digest === request.subject_digest,
          "conflict", "approval does not match the prepared operation and arguments");
        const pending = await m.one("SELECT COUNT(*) AS count FROM interactions WHERE session_id=? AND subject=? AND status IN ('pending','answered')", [identifier, request.subject]);
        need(pending.count === 0, "conflict", "operation already has an approval interaction");
      }
      m.change("INSERT INTO interactions(id,session_id,request,request_hash,expires_ms,subject,subject_digest) VALUES(?,?,?,?,?,?,?)",
        [interactionID, identifier, text[1], hash, request.expires_ms, request.subject, request.subject_digest]);
    }
    return result(m, "SELECT " + interactionColumns + " FROM interactions WHERE id=?", [interactionID], interaction);
  },
  async Interactions(db, [identifier, limit, after]) {
    return (await pageRows(db, interactionColumns, "interactions", "session_id=? AND id>?", "id", bytes("request")+"+"+bytes("answer"), limit, [identifier, after])).map(interaction);
  },
  async Answer(db, [identifier, interactionID, answer], text) {
    bounded(text[2], 65536, "interaction answer exceeds its bounded fields");
    const m = new Mutation(db);
    const current = await m.session(identifier, true);
    const prior = await m.one("SELECT " + interactionColumns + " FROM interactions WHERE id=? AND session_id=?", [interactionID, identifier]);
    const request = decode(prior.request);
    const status = answer.accept ? "answered" : "rejected";
    const answerText = field(text[2], "answer") ?? "";
    if (prior.status !== "pending") {
      need(prior.answer_key === answer.request_id && prior.status === status && prior.answer === answerText && answer.subject_digest === request.subject_digest,
        "conflict", "interaction was already resolved");
      return interaction(prior);
    }
    need(prior.revision === answer.revision && request.subject_digest === answer.subject_digest, "conflict", "stale interaction revision or subject digest");
    m.change(`UPDATE interactions SET status=?,revision=revision+1,answer=?,answer_key=? WHERE id=? AND session_id=? AND revision=?
AND status='pending' AND expires_ms>${clock}`, [status, answerText, answer.request_id, interactionID, identifier, answer.revision], true);
    await journal(m, current, "answered:" + interactionID + ":" + answer.request_id, "interaction.answered",
      { id: interactionID, status, subject: request.subject });
    return result(m, "SELECT " + interactionColumns + " FROM interactions WHERE id=?", [interactionID], interaction);
  },
  async Attach(db, [identifier, request], text) {
    bounded(request.resource_id ?? "", 1024, "connection resource exceeds its bound");
    const connectionID = await id("connection", identifier, request.request_id);
    const hash = await digest(text[1]);
    const m = new Mutation(db);
    const current = await m.session(identifier, true);
    const prior = await m.one("SELECT request_hash FROM connections WHERE id=?", [connectionID], true);
    if (prior) need(prior.request_hash === hash, "conflict", "connection key has different arguments");
    else {
      const template = (decode(current.config).connections ?? []).find(value => value.name === request.template);
      need(template, "forbidden", "connection template is not admitted");
      const count = await m.one("SELECT COUNT(*) AS count FROM connections WHERE session_id=? AND status='active'", [identifier]);
      need(count.count < 32, "overloaded", "active connection limit reached");
      const priorGeneration = await m.one("SELECT CAST(COALESCE(MAX(generation),0) AS TEXT) AS generation FROM connections WHERE session_id=? AND template=?", [identifier, request.template]);
      const generation = big(priorGeneration.generation) + 1n;
      m.change("INSERT INTO connections(id,session_id,template,generation,ownership,resource_id,request_hash) VALUES(?,?,?,?,?,?,?)",
        [connectionID, identifier, request.template, generation, template.ownership, request.resource_id ?? "", hash]);
    }
    return result(m, "SELECT " + connectionColumns + " FROM connections WHERE id=?", [connectionID], connection);
  },
  async Connections(db, [identifier, limit, after]) {
    return (await pageRows(db, connectionColumns, "connections", "session_id=? AND id>?", "id", "6*"+bytes("resource_id"), limit, [identifier, after])).map(connection);
  },
  async RevokeConnection(db, [identifier, connectionID]) {
    const m = new Mutation(db);
    await m.session(identifier, true);
    m.change("UPDATE connections SET status='revoked' WHERE id=? AND session_id=?", [connectionID, identifier], true);
    return result(m, "SELECT " + connectionColumns + " FROM connections WHERE id=?", [connectionID], connection);
  },
  async Documents(db, [identifier, scope, limit, after]) {
    return (await pageRows(db, documentColumns, "documents", `session_id=? AND scope=? AND key>? AND (expires_ms=0 OR expires_ms>${clock})`,
      "key", bytes("value"), limit, [identifier, scope, after])).map(document);
  },
  async PutDocument(db, [identifier, scope, key, update], text) {
    const hash = await digest(text[3]);
    const value = field(text[3], "value");
    bounded(value, 65536, "document exceeds its byte bound");
    need(["state", "memory"].includes(scope) && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(key), "invalid", "invalid document scope or key");
    if (update.expires_ms !== undefined) safe(update.expires_ms);
    const m = new Mutation(db);
    await m.session(identifier, true);
    const prior = await m.one("SELECT CAST(revision AS TEXT) AS revision,request_key,request_hash,LENGTH(CAST(value AS BLOB)) AS bytes FROM documents WHERE session_id=? AND scope=? AND key=?",
      [identifier, scope, key], true);
    if (prior?.request_key === update.request_id) need(prior.request_hash === hash, "conflict", "document key has different arguments");
    else {
      need(big(update.revision) === big(prior?.revision ?? "0"), "conflict", "document revision changed");
      await documentQuota(m, identifier, scope, key, value);
      m.change(`INSERT INTO documents(session_id,scope,key,revision,value,expires_ms,request_key,request_hash) VALUES(?,?,?,?,?,?,?,?)
ON CONFLICT(session_id,scope,key) DO UPDATE SET revision=excluded.revision,value=excluded.value,expires_ms=excluded.expires_ms,
request_key=excluded.request_key,request_hash=excluded.request_hash`,
      [identifier, scope, key, big(prior?.revision ?? "0") + 1n, value, update.expires_ms ?? 0, update.request_id, hash]);
    }
    return result(m, "SELECT " + documentColumns + " FROM documents WHERE session_id=? AND scope=? AND key=?", [identifier, scope, key], document);
  },
  async DeleteDocument(db, [identifier, scope, key]) {
    const m = new Mutation(db);
    await m.session(identifier, true);
    const before = await m.one("SELECT " + documentColumns + " FROM documents WHERE session_id=? AND scope=? AND key=?", [identifier, scope, key]);
    m.change("DELETE FROM documents WHERE session_id=? AND scope=? AND key=?", [identifier, scope, key], true);
    await m.commit();
    return document(before);
  },
  async PutFile(db, [identifier, file]) {
    bounded(file.name, 128, "file name exceeds its bound");
    bounded(file.media_type, 256, "file media type exceeds its bound");
    bounded(file.provenance, 128, "file provenance exceeds its bound");
    const m = new Mutation(db);
    await m.session(identifier, true);
    const totals = await m.one("SELECT COUNT(*) AS count,COALESCE(SUM(size),0) AS bytes FROM files WHERE session_id=?", [identifier]);
    const prior = await m.one("SELECT size FROM files WHERE session_id=? AND name=?", [identifier, file.name], true);
    need((prior || totals.count < 1024) && totals.bytes - (prior?.size ?? 0) + safe(file.size, 0, 16 * 1024 * 1024) <= 1024 * 1024 * 1024,
      "overloaded", "file quota reached");
    m.change(`INSERT INTO files(session_id,name,digest,size,media_type,provenance) VALUES(?,?,?,?,?,?)
ON CONFLICT(session_id,name) DO UPDATE SET digest=excluded.digest,size=excluded.size,media_type=excluded.media_type,provenance=excluded.provenance`,
    [identifier, file.name, file.digest, file.size, file.media_type, file.provenance]);
    await m.commit();
  },
  async Files(db, [identifier, limit, after]) {
    return pageRows(db, fileColumns, "files", "session_id=? AND name>?", "name", "6*"+bytes("provenance"), limit, [identifier, after]);
  },
  async DeleteFile(db, [identifier, name]) {
    const m = new Mutation(db);
    await m.session(identifier, true);
    const before = await m.one("SELECT " + fileColumns + " FROM files WHERE session_id=? AND name=?", [identifier, name]);
    m.change("DELETE FROM files WHERE session_id=? AND name=?", [identifier, name], true);
    await m.commit();
    return before;
  },
  async Handoff(db, [identifier, request], text) {
    const m = new Mutation(db);
    const current = await m.session(identifier);
    const hash = await digest(text[1]);
    const prior = await m.one("SELECT request_hash FROM harness_handoffs WHERE session_id=? AND request_key=?", [identifier, request.request_id], true);
    if (prior) {
      need(prior.request_hash === hash, "conflict", "handoff request id has different arguments");
      return session(current);
    }
    await m.unfrozen(identifier);
    need(current.lifecycle === "open", "conflict", "session is not open");
    await m.lineage(identifier);
    need(current.revision === request.revision, "conflict", "session revision changed");
    const config = decode(current.config);
    const admitted = (config.harness_transitions ?? []).some(value => equal(value, request.harness));
    need(admitted, "forbidden", "harness transition is not admitted");
    const settled = `NOT EXISTS(SELECT 1 FROM runs WHERE session_id=? AND status IN ('queued','active','waiting','paused','blocked'))
AND NOT EXISTS(SELECT 1 FROM commands WHERE session_id=? AND status='accepted' AND kind IN ('message','action','event'))`;
    const active = await m.one("SELECT CASE WHEN " + settled + " THEN 0 ELSE 1 END AS count", [identifier, identifier]);
    need(active.count === 0, "conflict", "harness handoff requires settled runs and accepted work");
    m.guard(settled, [identifier, identifier]);
    const required = await capability("harness", request.harness.name, request.harness.version, config.model.provider);
    const liveOwner = await m.one(`SELECT COUNT(*) AS count FROM nodes WHERE id=? AND lease>${clock} AND draining=0`, [current.owner]);
    if (liveOwner.count !== 0) {
      const available = await m.one("SELECT COUNT(*) AS count FROM node_capabilities WHERE node_id=? AND capability=?", [current.owner, required]);
      need(available.count === 1, "unsupported", "live owner cannot continue the admitted harness transition");
    }
    await journal(m, current, "handoff:" + await id(identifier, request.request_id), "harness.handed_off",
      { request_id: request.request_id, from: config.harness, to: request.harness, prior_revision: request.revision });
    config.harness = request.harness;
    m.change("UPDATE sessions SET config=?,revision=revision+1 WHERE id=? AND revision=?", [encode(config), identifier, request.revision], true);
    m.change("DELETE FROM session_requirements WHERE session_id=? AND capability LIKE 'harness:%'", [identifier]);
    m.change("INSERT INTO session_requirements VALUES(?,?)", [identifier, required]);
    m.change("INSERT INTO harness_handoffs VALUES(?,?,?)", [identifier, request.request_id, hash]);
    return result(m, "SELECT " + sessionColumns + " FROM sessions WHERE id=?", [identifier], session);
  },
  async Delete(db, [identifier]) {
    const m = new Mutation(db);
    const current = await m.session(identifier);
    if (current.lifecycle !== "deleted") {
      await cancelLineage(m, identifier);
      m.change("UPDATE sessions SET lifecycle='deleted',generation=generation+1,revision=revision+1 WHERE id=?", [identifier]);
      for (const sql of [
        "UPDATE runs SET status='cancelled' WHERE session_id=? AND status IN ('queued','active','waiting','paused','blocked')",
        "UPDATE commands SET status='cancelled' WHERE session_id=? AND status='accepted'",
        "UPDATE schedules SET status='cancelled' WHERE session_id=?",
        "UPDATE participants SET revoked=1 WHERE session_id=?",
        "UPDATE connections SET status='revoked' WHERE session_id=?",
        "UPDATE interactions SET status='cancelled' WHERE session_id=? AND status='pending'",
      ]) m.change(sql, [identifier]);
      m.change(wakeParents, [identifier]);
    }
    return result(m, "SELECT " + sessionColumns + " FROM sessions WHERE id=?", [identifier], session);
  },
  async PublishCoverage(db, [owner, coverage]) {
    const m = new Mutation(db);
    const current = await m.owner(coverage.Session, owner);
    need(current.generation === String(coverage.Generation), "conflict", "archive deletion generation changed");
    const members = coverage.Records;
    need(Array.isArray(members) && members.length > 0 && members.length <= 128, "invalid", "coverage requires bounded exact membership");
    let previous = 0n;
    for (const member of members) {
      const sequence = big(member.Sequence);
      need(sequence > previous && sequence >= big(coverage.First) && sequence <= big(coverage.Last), "invalid", "invalid archive membership order");
      previous = sequence;
    }
    const membersText = encode(members.map(member => ({ Sequence: member.Sequence, Digest: member.Digest })));
    m.guard(`(SELECT COUNT(*) FROM allocations a JOIN json_each(?) member ON a.sequence=json_extract(member.value,'$.Sequence')
AND a.digest=json_extract(member.value,'$.Digest') WHERE a.session_id=? AND a.status='written')=?`, [membersText, coverage.Session, members.length]);
    m.change("INSERT INTO coverage VALUES(?,?,?,?,?,?,?) ON CONFLICT DO NOTHING",
      [coverage.Session, coverage.First, coverage.Last, coverage.Key, coverage.Digest, coverage.Bytes, coverage.Generation]);
    m.change(`UPDATE allocations SET archive_key=? WHERE session_id=? AND status='written' AND EXISTS(
SELECT 1 FROM json_each(?) member WHERE sequence=json_extract(member.value,'$.Sequence') AND digest=json_extract(member.value,'$.Digest'))`,
    [coverage.Key, coverage.Session, membersText]);
    m.change(`UPDATE operations SET result_body='',arguments=CASE WHEN EXISTS(SELECT 1 FROM allocations a WHERE a.session_id=operations.session_id
AND a.sequence=operations.intent_sequence AND a.archive_key<>'') THEN '' ELSE arguments END
WHERE session_id=? AND status IN ('completed','failed') AND EXISTS(SELECT 1 FROM json_each(?) member
WHERE result_sequence=json_extract(member.value,'$.Sequence') AND result_digest=json_extract(member.value,'$.Digest'))`, [coverage.Session, membersText]);
    m.change(`UPDATE operations SET arguments='' WHERE session_id=? AND status IN ('completed','failed') AND EXISTS(
SELECT 1 FROM json_each(?) member WHERE intent_sequence=json_extract(member.value,'$.Sequence') AND intent_digest=json_extract(member.value,'$.Digest'))`, [coverage.Session, membersText]);
    await m.commit();
  },
  async Coverage(db, [identifier, after, limit]) {
    return (await rows(db, `SELECT c.session_id,CAST(c.first_sequence AS TEXT) AS first_sequence,CAST(c.last_sequence AS TEXT) AS last_sequence,
c.object_key,c.digest,c.bytes,CAST(c.generation AS TEXT) AS generation FROM coverage c JOIN allocations a
ON a.session_id=c.session_id AND a.archive_key=c.object_key WHERE c.session_id=? AND a.sequence=? LIMIT ?`,
    [identifier, big(after) + 1n, safe(limit, 1, 128)])).map(value => ({
      Session: value.session_id, First: integer(value.first_sequence), Last: integer(value.last_sequence), Key: value.object_key,
      Digest: value.digest, Bytes: value.bytes, Generation: integer(value.generation),
    }));
  },
  async PutPackage(db, [tenant, pkg], text) {
    const m = new Mutation(db);
    await m.scope("package:" + tenant + ":" + pkg.id);
    m.change("INSERT INTO packages(tenant,id,body) VALUES(?,?,?) ON CONFLICT DO NOTHING", [tenant, pkg.id, text[1]]);
    await m.commit();
  },
  async Package(db, [tenant, identifier]) {
    return raw((await one(db, "SELECT body FROM packages WHERE tenant=? AND id=?", [tenant, identifier])).body);
  },
};
