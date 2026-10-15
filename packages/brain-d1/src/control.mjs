import {
  Fault, Mutation, big, capability, clock, command, commandColumns, decode, digest, encode, field, id, identifiers,
  integer, maximumInteger, need, one, raw, result, rows, run, runColumns, safe, session, sessionColumns, wire,
} from "./domain.mjs";
import { bytes, pageRows, runBytes } from "./pages.mjs";
import { documentQuota } from "./quotas.mjs";
import { createBudget, validateDelegation } from "./budgets.mjs";
import { cancelLineage, pauseLineage } from "./lineage.mjs";
import { admitChild, childrenReady, delegations, guardChildWait, wakeParents } from "./children.mjs";

export const readyPredicate = `EXISTS(SELECT 1 FROM ownership_transfers t WHERE t.session_id=sessions.id AND t.phase IN ('draining','verifying'))
OR EXISTS(SELECT 1 FROM commands c WHERE c.session_id=sessions.id AND c.status='accepted')
OR EXISTS(SELECT 1 FROM operations o WHERE o.session_id=sessions.id AND o.result_sequence=0 AND o.status IN ('completed','failed','unknown'))
OR (sessions.paused=0 AND EXISTS(SELECT 1 FROM runs r WHERE r.session_id=sessions.id AND (
r.status IN ('queued','active') OR (r.status='waiting' AND (
(r.wait_kind='operation' AND EXISTS(SELECT 1 FROM operations o WHERE o.id=r.wait_id AND o.status IN ('completed','failed','unknown')))
OR (r.wait_kind='interaction' AND EXISTS(SELECT 1 FROM interactions i WHERE i.id=r.wait_id AND i.status<>'pending'))
OR (r.wait_kind='child' AND NOT EXISTS(SELECT 1 FROM runs child WHERE child.session_id=r.wait_id AND child.status IN ('queued','active','waiting','paused'))
AND NOT EXISTS(SELECT 1 FROM commands child WHERE child.session_id=r.wait_id AND child.status='accepted' AND child.kind IN ('message','action','event')))
OR (${childrenReady})
)))))`;

export async function acceptInput(m, principal, current, request, dataText) {
  const identifier = await id("input", current.id, request.request_id);
  const hash = await digest(dataText);
  const previous = await m.one("SELECT digest FROM commands WHERE id=? AND session_id=?", [identifier, current.id], true);
  if (previous) {
    need(previous.digest === hash, "conflict", "input idempotency key has different content");
    return identifier;
  }
  need(current.tenant === principal.Tenant && current.lifecycle === "open", "conflict", "session is not open");
  if (current.budget && !["control", "journal"].includes(request.data.kind)) await m.lineage(current.id);
  const config = decode(current.config);
  need(new TextEncoder().encode(dataText).length <= safe(config.limits.input_bytes), "invalid", "input exceeds its admitted byte limit");
  const count = await m.one("SELECT COUNT(*) AS count FROM commands WHERE session_id=? AND status='accepted'", [current.id]);
  m.pending ??= new Map();
  const pending = m.pending.get(current.id) ?? 0;
  need(count.count + pending < safe(config.limits.pending_inputs), "overloaded", "pending input limit reached");
  m.change(`INSERT INTO commands(id,session_id,request_key,digest,payload,source,kind,accepted_ms) VALUES(?,?,?,?,?,?,?,${clock})`,
    [identifier, current.id, request.request_id, hash, dataText, principal.Subject, request.data.kind]);
  m.pending.set(current.id, pending + 1);
  if (request.data.kind === "control") {
    switch (request.data.control) {
      case "pause":
        await pauseLineage(m, current.id, true);
        m.change("UPDATE sessions SET paused=1,revision=revision+1 WHERE id=? AND paused=0", [current.id]);
        break;
      case "resume":
        await pauseLineage(m, current.id, false);
        m.change("UPDATE sessions SET paused=0,revision=revision+1 WHERE id=? AND paused=1", [current.id]);
        m.change(`UPDATE runs SET status=CASE WHEN decision='' AND wait_kind<>'' THEN 'waiting' ELSE 'active' END,error='',updated_ms=${clock}
WHERE session_id=? AND status='blocked'`, [current.id]);
        break;
      case "cancel":
      case "close":
        await cancelLineage(m, current.id);
        m.change(`UPDATE runs SET status='cancelled',updated_ms=${clock} WHERE session_id=? AND status IN ('queued','active','waiting','paused','blocked')`, [current.id]);
        m.change("UPDATE commands SET status='cancelled' WHERE session_id=? AND status='accepted' AND kind IN ('message','action','event')", [current.id]);
        m.change("UPDATE interactions SET status='cancelled',revision=revision+1 WHERE session_id=? AND status='pending'", [current.id]);
        m.change(wakeParents, [current.id]);
        if (request.data.control === "close") {
          m.change("UPDATE sessions SET lifecycle='closed',revision=revision+1 WHERE id=? AND lifecycle='open'", [current.id]);
          m.change("UPDATE schedules SET status='cancelled' WHERE session_id=? AND status='active'", [current.id]);
        }
        break;
      default: need(false, "invalid", "unknown control");
    }
  }
  m.change(`UPDATE sessions SET ready=1,ready_ms=CASE WHEN ready=0 THEN ${clock} ELSE ready_ms END WHERE id=?`, [current.id]);
  return identifier;
}

export async function journal(m, current, requestID, name, data) {
  const dataText = wire("InputData", { kind: "journal", data, name });
  return acceptInput(m, { Tenant: current.tenant, Subject: "kernel" }, current,
    { request_id: requestID, data: decode(dataText) }, dataText);
}

const eligibility = `NOT EXISTS(SELECT 1 FROM session_requirements r WHERE r.session_id=?
AND NOT EXISTS(SELECT 1 FROM node_capabilities c WHERE c.node_id=n.id AND c.capability=r.capability))`;

export const controlHandlers = {
  async Clock(db) { return (await one(db, `SELECT ${clock} AS now`)).now; },
  async RegisterNode(db, [node, lease]) {
    safe(lease, 1000, 3600000);
    const m = new Mutation(db);
    await m.scope("node:" + node);
    m.change(`INSERT INTO nodes(id,lease) VALUES(?,${clock}+?)`, [node, lease]);
    await m.commit();
  },
  async ConfigureNode(db, [node, endpoint, custody, values]) {
    const capabilities = identifiers(values ?? []);
    const configuration = await digest(encode({ Endpoint: endpoint, Capabilities: capabilities }));
    const m = new Mutation(db);
    await m.scope("node:" + node);
    const retained = await m.one("SELECT value FROM cluster_configuration WHERE key='custody'", [], true);
    need(!retained || retained.value === custody, "forbidden", "cluster requires its original retained custody key");
    m.change("INSERT INTO cluster_configuration VALUES('custody',?) ON CONFLICT DO NOTHING", [custody]);
    m.guard("NOT EXISTS(SELECT 1 FROM cluster_configuration WHERE key='custody' AND value<>?)", [custody]);
    m.change(`UPDATE nodes SET endpoint=?,configuration_digest=? WHERE id=? AND (configuration_digest='' OR configuration_digest=?)
AND lease>${clock} AND draining=0`, [endpoint, configuration, node, configuration], true);
    m.change("INSERT INTO node_capabilities SELECT ?,value FROM json_each(?) WHERE 1 ON CONFLICT DO NOTHING", [node, encode(capabilities)]);
    await m.commit();
  },
  async RenewNode(db, [node, lease]) {
    safe(lease, 1000, 3600000);
    const m = new Mutation(db);
    await m.scope("node:" + node);
    m.change(`UPDATE nodes SET lease=${clock}+? WHERE id=? AND lease>${clock} AND draining=0`, [lease, node], true);
    await m.commit();
  },
  async RetireNode(db, [node]) {
    const m = new Mutation(db);
    await m.scope("node:" + node);
    m.change("UPDATE nodes SET draining=1,lease=0 WHERE id=?", [node]);
    await m.commit();
  },
  async Delegate(db, [parent, owner, runID, step, index, request, requirements], text) {
    safe(index, 0, 7);
    request = { ...request, request_id: await id("child", parent, await id("delegation", parent, runID, String(step), String(index))) };
    const current = await one(db, "SELECT " + sessionColumns + " FROM sessions WHERE id=?", [parent]);
    const args = [current.tenant, request, parent, current.budget, requirements];
    const encoded = [encode(current.tenant), wire("SessionCreate", request), encode(parent), encode(current.budget), text[6]];
    return controlHandlers.Create({ ...db, origin: { owner, runID, step, index } }, args, encoded);
  },
  async Create(db, [tenant, request, parent, sharedBudget, required], text) {
    const identifier = await id("session", tenant, request.request_id);
    const hash = await digest(parent ? encode([raw(text[1]), parent, sharedBudget]) : text[1]);
    const m = new Mutation(db);
    await m.scope("session:" + identifier);
    let ancestor;
    if (parent) {
      ancestor = await m.session(parent);
      need(ancestor.tenant === tenant && ancestor.budget === sharedBudget && ancestor.lifecycle === "open",
        "forbidden", "delegation parent or shared authority is not live");
      await m.lineage(parent);
      if (db.origin) {
        await admitChild(m, parent, db.origin, request);
      }
      validateDelegation(decode(ancestor.config), request.config);
    } else need(!sharedBudget, "invalid", "a shared budget requires its admitted parent");
    const prior = await m.one("SELECT create_hash FROM sessions WHERE tenant=? AND create_key=?", [tenant, request.request_id], true);
    if (prior) {
      need(prior.create_hash === hash, "conflict", "idempotency key has different session arguments");
      if (db.origin) {
        const link = await m.one("SELECT child_id FROM run_children WHERE session_id=? AND run_id=? AND step=? AND ordinal=?",
          [parent, db.origin.runID, db.origin.step, db.origin.index], true);
        need(link?.child_id === identifier, "conflict", "child identity does not match its original decision slot");
      }
      return session(await m.one("SELECT " + sessionColumns + " FROM sessions WHERE id=?", [identifier]));
    }
    const requirements = identifiers(required ?? []);
    const configText = field(text[1], "config");
    const budget = sharedBudget || identifier;
    await createBudget(m, identifier, request, parent);
    m.change(`INSERT INTO sessions(id,tenant,create_key,create_hash,config,created_ms,parent,budget) VALUES(?,?,?,?,?,${clock},?,?)`,
      [identifier, tenant, request.request_id, hash, configText, parent, budget]);
    if (parent) await journal(m, ancestor, "delegated:" + identifier, "session.delegated", { child_id: identifier, shared_budget: budget,
      ...(db.origin ? { run_id: db.origin.runID, step: db.origin.step, index: db.origin.index } : {}) });
    m.change("INSERT INTO session_requirements SELECT ?,value FROM json_each(?)", [identifier, encode(requirements)]);
    if (request.message !== undefined && request.message !== null) {
      const input = { request_id: request.request_id + ":initial", data: { kind: "message", text: request.message } };
      await acceptInput(m, { Tenant: tenant, Subject: "creator" }, { id: identifier, tenant, lifecycle: "open", config: configText },
        input, wire("InputData", input.data));
    }
    if (db.origin) m.change("INSERT INTO run_children VALUES(?,?,?,?,?,?)",
      [parent, db.origin.runID, db.origin.step, db.origin.index, identifier, await id("input", identifier, request.request_id + ":initial")]);
    return result(m, "SELECT " + sessionColumns + " FROM sessions WHERE id=?", [identifier], session);
  },
  async Session(db, [tenant, identifier]) {
    const value = await one(db, "SELECT " + sessionColumns + " FROM sessions WHERE id=?", [identifier]);
    need(value.lifecycle !== "deleted" && (!tenant || value.tenant === tenant), "not_found", "session not found");
    return session(value);
  },
  async List(db, [tenant, after, limit]) {
    return (await pageRows(db, sessionColumns, "sessions", "tenant=? AND id>? AND lifecycle<>'deleted'", "id", bytes("config"), limit, [tenant, after])).map(session);
  },
  async ClaimOwner(db, [identifier, node]) {
    const m = new Mutation(db);
    const current = await m.session(identifier);
    const eligible = `EXISTS(SELECT 1 FROM nodes n WHERE n.id=? AND n.lease>${clock} AND n.draining=0 AND ${eligibility})`;
    const valid = await m.one("SELECT CASE WHEN " + eligible + " THEN 1 ELSE 0 END AS valid", [node, identifier]);
    need(valid.valid === 1, "conflict", "node has no live eligible lease for the retained resources");
    m.guard(eligible, [node, identifier]);
    if (current.owner !== node) {
      const epoch = big(current.epoch) + 1n;
      need(epoch <= maximumInteger, "overloaded", "ownership epoch exhausted");
      m.guard(`NOT EXISTS(SELECT 1 FROM nodes WHERE id=? AND lease>${clock} AND draining=0)`, [current.owner]);
      m.change(`UPDATE ownership_transfers SET phase='failed',error=?,updated_ms=${clock}
WHERE session_id=? AND phase IN ('draining','verifying')`, [
        encode({ code: "uncertain", message: "source incarnation was lost before planned transfer completed", retryable: false }), identifier,
      ]);
      m.change("UPDATE allocations SET status='unresolved' WHERE session_id=? AND status='reserved' AND epoch<?", [identifier, epoch]);
      m.change("UPDATE sessions SET owner=?,epoch=? WHERE id=? AND epoch=? AND lifecycle<>'deleted'", [node, epoch, identifier, current.epoch], true);
    }
    return result(m, "SELECT owner,CAST(epoch AS TEXT) AS epoch FROM sessions WHERE id=?", [identifier],
      value => ({ Node: value.owner, Epoch: integer(value.epoch) }));
  },
  async Accept(db, [principal, identifier, request], text) {
    const m = new Mutation(db);
    const current = await m.session(identifier);
    const commandID = await acceptInput(m, principal, current, request, field(text[2], "data"));
    return result(m, "SELECT " + commandColumns + " FROM commands WHERE id=?", [commandID], command);
  },
  async Command(db, [identifier, commandID]) {
    return command(await one(db, "SELECT " + commandColumns + " FROM commands WHERE session_id=? AND id=?", [identifier, commandID]));
  },
  async Pending(db, [node, limit]) {
    return (await rows(db, `SELECT id FROM (
SELECT s.id,s.ready_ms,ROW_NUMBER() OVER(PARTITION BY s.tenant ORDER BY s.ready_ms,s.id) AS tenant_turn
FROM sessions s WHERE s.ready=1 AND s.lifecycle<>'deleted'
AND (NOT EXISTS(SELECT 1 FROM ownership_transfers t WHERE t.session_id=s.id AND t.phase IN ('draining','verifying'))
OR NOT EXISTS(SELECT 1 FROM nodes n WHERE n.id=s.owner AND n.lease>${clock} AND n.draining=0))
AND NOT EXISTS(SELECT 1 FROM session_requirements r WHERE r.session_id=s.id
AND NOT EXISTS(SELECT 1 FROM node_capabilities c WHERE c.node_id=? AND c.capability=r.capability))
AND (s.owner=? OR NOT EXISTS(SELECT 1 FROM nodes n WHERE n.id=s.owner AND n.lease>${clock} AND n.draining=0))
) ready_sessions ORDER BY tenant_turn,ready_ms,id LIMIT ?`, [node, node, safe(limit, 1, 16384)])).map(value => value.id);
  },
  async FinishStep(db, [identifier, owner]) {
    const m = new Mutation(db);
    await m.owner(identifier, owner);
    m.change(`UPDATE sessions SET ready=CASE WHEN ${readyPredicate} THEN 1 ELSE 0 END,ready_ms=${clock} WHERE id=?`, [identifier]);
    await m.commit();
  },
  async NextInput(db, [identifier, owner]) {
    const m = new Mutation(db);
    await m.owner(identifier, owner);
    await m.unfrozen(identifier);
    return result(m, "SELECT id,request_key,payload,source FROM commands WHERE session_id=? AND status='accepted' ORDER BY accepted_ms,id LIMIT 1",
      [identifier], value => [{ request_id: value.request_key, data: raw(value.payload) }, value.id, value.source]);
  },
  async ApplyInput(db, [identifier, apply]) {
    const m = new Mutation(db);
    await m.owner(identifier, apply.Owner);
    const current = await m.one("SELECT payload,status FROM commands WHERE id=? AND session_id=?", [apply.Command, identifier]);
    if (current.status !== "accepted") return;
    const input = decode(current.payload);
    if (apply.Disposition === "state") {
      const value = field(current.payload, "data");
      try {
        await documentQuota(m, identifier, "state", apply.StateKey, value);
      } catch (error) {
        if (!(error instanceof Fault) || !["invalid", "overloaded"].includes(error.code)) throw error;
        m.change("UPDATE commands SET status='failed',sequence=?,error=? WHERE id=? AND status='accepted'",
          [apply.Sequence, encode({ code: error.code, message: error.message, retryable: error.code === "overloaded" }), apply.Command], true);
        m.change(wakeParents, [identifier]);
        await m.commit();
        return;
      }
      m.change(`INSERT INTO documents(session_id,scope,key,revision,value,request_key,request_hash)
VALUES(?,'state',?,1,?,?,?) ON CONFLICT(session_id,scope,key) DO UPDATE SET revision=documents.revision+1,
value=excluded.value,request_key=excluded.request_key,request_hash=excluded.request_hash`,
      [identifier, apply.StateKey, value, apply.Command, await digest(value)]);
    }
    if (apply.Disposition === "interrupt") {
      await cancelLineage(m, identifier);
      m.change("UPDATE runs SET status='cancelled' WHERE session_id=? AND status IN ('queued','active','waiting','paused','blocked')", [identifier]);
      m.change(wakeParents, [identifier]);
    }
    let runID = "";
    if (apply.Run) {
      runID = await id("run", identifier, apply.Command);
      m.change(`INSERT INTO runs(id,session_id,command_id,status,updated_ms) VALUES(?,?,?,'queued',${clock}) ON CONFLICT DO NOTHING`,
        [runID, identifier, apply.Command]);
    }
    if (apply.Disposition === "steer" || apply.Disposition === "wake") {
      m.change("UPDATE runs SET status='active',wait_kind='',wait_id='' WHERE session_id=? AND status='waiting' AND wait_kind='input'", [identifier]);
    }
    need(input.kind, "corrupt", "input kind is missing");
    m.change("UPDATE commands SET status='applied',sequence=?,run_id=? WHERE id=? AND status='accepted'",
      [apply.Sequence, runID, apply.Command], true);
    await m.commit();
  },
  async Allocate(db, [identifier, owner]) {
    const m = new Mutation(db);
    const current = await m.owner(identifier, owner);
    await m.unfrozen(identifier, true);
    const sequence = big(current.sequence) + 1n;
    need(sequence <= maximumInteger, "overloaded", "record sequence exhausted");
    m.change("UPDATE sessions SET sequence=sequence+1 WHERE id=? AND sequence<9223372036854775807", [identifier], true);
    m.change("INSERT INTO allocations(session_id,sequence,owner,epoch) VALUES(?,?,?,?)", [identifier, sequence, owner.Node, owner.Epoch]);
    await m.commit();
    return integer(sequence);
  },
  async Written(db, [identifier, owner, sequence, hash, bytes]) {
    const m = new Mutation(db);
    await m.owner(identifier, owner);
    safe(bytes, 1, 262144);
    m.change(`UPDATE allocations SET status='written',digest=?,bytes=? WHERE session_id=? AND sequence=? AND owner=? AND epoch=?
AND (status='reserved' OR (status='written' AND digest=?))`, [hash, bytes, identifier, sequence, owner.Node, owner.Epoch, hash], true);
    await m.commit();
  },
  async SealUnused(db, [identifier, owner, sequence]) {
    const m = new Mutation(db);
    await m.owner(identifier, owner);
    m.change("UPDATE allocations SET status='unused' WHERE session_id=? AND sequence=? AND owner=? AND epoch=? AND status='reserved'",
      [identifier, sequence, owner.Node, owner.Epoch], true);
    await m.commit();
  },
  async Allocations(db, [identifier, after, limit]) {
    const query = db.withSession("first-primary");
    const high = await one(query, "SELECT CAST(sequence AS TEXT) AS sequence FROM sessions WHERE id=?", [identifier]);
    const values = await rows(query, `SELECT CAST(sequence AS TEXT) AS sequence,status,digest,bytes FROM allocations
WHERE session_id=? AND sequence>? AND sequence<=? ORDER BY sequence LIMIT ?`, [identifier, after, high.sequence, safe(limit, 1, 1024)]);
    return [values.map(value => ({ Sequence: integer(value.sequence), Status: value.status, Digest: value.digest, Bytes: value.bytes })), integer(high.sequence)];
  },
  async LocateRecord(db, [identifier, sequence]) {
    const value = await one(db, `SELECT CAST(a.sequence AS TEXT) AS sequence,a.status,a.digest,a.bytes,a.owner,COALESCE(n.endpoint,'') AS endpoint,s.owner AS current_owner
FROM allocations a JOIN sessions s ON s.id=a.session_id LEFT JOIN nodes n ON n.id=a.owner WHERE a.session_id=? AND a.sequence=?`, [identifier, sequence]);
    return { Sequence: integer(value.sequence), Status: value.status, Digest: value.digest, Bytes: value.bytes, Node: value.owner, Endpoint: value.endpoint, CurrentOwner: value.current_owner };
  },
  async Runs(db, [identifier, limit, after]) {
    return (await pageRows(db, runColumns, "runs r JOIN commands c ON c.id=r.command_id", "r.session_id=? AND r.id>?", "r.id", runBytes, limit, [identifier, after])).map(run);
  },
  async BeginDecision(db, [identifier, owner, runID, step, hash, process, repeatable]) {
    safe(step, 0, 4294967295);
    need(typeof hash === "string" && /^[a-f0-9]{64}$/u.test(hash) && typeof process === "string" &&
      /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(process) && typeof repeatable === "boolean",
    "invalid", "invalid harness decision boundary");
    const m = new Mutation(db);
    await m.owner(identifier, owner, true);
    const current = await m.one(`SELECT decision_started_step,decision_input_hash,decision_repeatable FROM runs
WHERE session_id=? AND id=? AND step=? AND decision='' AND status IN ('queued','active','waiting')`, [identifier, runID, step], true);
    need(current, "conflict", "run no longer holds this decision boundary");
    if (big(current.decision_started_step) === big(step)) {
      need(current.decision_input_hash === hash, "corrupt", "harness decision request diverged from its original boundary");
      need(current.decision_repeatable !== 0 && repeatable, "unsupported", "harness decision was already admitted without repeatable continuation");
    }
    m.change(`UPDATE runs SET harness_process=?,decision_started_step=?,decision_input_hash=?,decision_repeatable=?
WHERE session_id=? AND id=? AND step=? AND decision='' AND status IN ('queued','active','waiting')`,
    [process, step, hash, repeatable, identifier, runID, step], true);
    await m.commit();
  },
  async SaveHarnessDecision(db, [identifier, owner, runID, step, decision, process], text) {
    need(typeof process === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(process), "invalid", "harness process identity is invalid");
    return controlHandlers.SaveDecision({ ...db, harnessProcess: process }, [identifier, owner, runID, step, decision], text);
  },
  async SaveDecision(db, [identifier, owner, runID, step, decision], text) {
    const m = new Mutation(db);
    await m.owner(identifier, owner, true);
    need(new TextEncoder().encode(text[4]).length <= 262144, "invalid", "harness decision exceeds its durable checkpoint bound");
    m.change(`UPDATE runs SET decision=?,status='active',harness_process=CASE WHEN ?='' THEN harness_process ELSE ? END,updated_ms=${clock}
WHERE id=? AND session_id=? AND step=? AND status IN ('queued','active','waiting') AND (decision='' OR decision=?)
AND (?='' OR (harness_process=? AND decision_started_step=?))`,
    [text[4], db.harnessProcess ?? "", db.harnessProcess ?? "", runID, identifier, step, text[4], db.harnessProcess ?? "", db.harnessProcess ?? "", step], true);
    if (["delegate", "fork"].includes(decision.kind)) {
      m.change("INSERT INTO child_groups VALUES(?,?,?,?) ON CONFLICT DO NOTHING", [identifier, runID, step, delegations(decision).length]);
    }
    await m.commit();
  },
  async AdvanceRun(db, [identifier, owner, runID, step, status, wait, failure], text) {
    need(status !== "completed", "invalid", "completion requires written matching evidence");
    safe(step, 0, 4294967294);
    const m = new Mutation(db);
    await m.owner(identifier, owner);
    if (status === "blocked") {
      m.change(`UPDATE runs SET status='blocked',error=?,updated_ms=${clock} WHERE id=? AND session_id=? AND step=?
AND status IN ('queued','active','waiting','paused')`, [failure ? text[6] : "", runID, identifier, step], true);
      m.change(wakeParents, [identifier]);
    } else {
      const current = await m.one("SELECT decision,checkpoint FROM runs WHERE id=? AND session_id=?", [runID, identifier]);
      await guardChildWait(m, identifier, runID, step, current.decision, wait);
      const checkpoint = current.decision ? field(current.decision, "checkpoint") ?? "null" : current.checkpoint;
      m.change(`UPDATE runs SET status=?,step=step+1,checkpoint=?,decision='',wait_kind=?,wait_id=?,error=?,updated_ms=${clock}
WHERE id=? AND session_id=? AND step=? AND status NOT IN ('completed','failed','cancelled','blocked') AND step<9223372036854775807`,
      [status, checkpoint, wait?.kind ?? "", wait?.id ?? "", failure ? text[6] : "", runID, identifier, step], true);
      if (["completed", "failed", "cancelled", "blocked"].includes(status)) {
        m.change(wakeParents, [identifier]);
      }
    }
    await m.commit();
  },
  async Report(db) {
    const totals = await one(db, `SELECT CAST(COALESCE(SUM(b.retained),0) AS TEXT) AS retained,CAST(COALESCE(SUM(b.reserved),0) AS TEXT) AS reserved
FROM budgets b JOIN sessions s ON s.id=b.id WHERE s.parent=''`);
    const count = await one(db, "SELECT COUNT(*) AS count FROM sessions WHERE lifecycle<>'deleted'");
    return { format: "brain-data/4", retained_bytes: integer(totals.retained), reserved_output_bytes: integer(totals.reserved),
      cache_bytes: 0, unarchived_bytes: 0, archive_queue_bytes: 0, oldest_unarchived_ms: 0, sessions: count.count, accepting: true };
  },
};
