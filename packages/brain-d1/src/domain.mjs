import { LosslessNumber, isLosslessNumber, parse } from "lossless-json";
import schema from "../../../internal/generated/wire/schema.json";
import sqlSchema from "../../../adapters/sqlcontrol/schema.sql";

export const clock = "CAST(unixepoch('subsec')*1000 AS INTEGER)";
export const maximumInteger = 9223372036854775807n;
export class Fault extends Error {
  constructor(code, message) { super(message); this.code = code; }
}
export class ScopeChanged extends Fault {
  constructor() { super("conflict", "D1 atomic-plan read scope changed before commit"); }
}
export function need(condition, code, message) {
  if (!condition) throw new Fault(code, message);
}
export class Raw {
  constructor(text) { this.text = text; }
}
export const raw = text => new Raw(text || "null");
export const decode = text => parse(text);
export const integer = value => new LosslessNumber(String(value));
export function big(value) {
  const text = isLosslessNumber(value) ? value.value : String(value);
  need(/^-?(0|[1-9][0-9]*)$/u.test(text), "invalid", "integer is not canonical");
  const number = BigInt(text);
  need(number >= -maximumInteger && number <= maximumInteger, "invalid", "integer exceeds the SQL domain");
  return number;
}
export function safe(value, minimum = 0, maximum = Number.MAX_SAFE_INTEGER) {
  const number = Number(big(value));
  need(Number.isSafeInteger(number) && number >= minimum && number <= maximum, "invalid", "integer exceeds its admitted bound");
  return number;
}
export function encode(value) {
  if (value instanceof Raw) return value.text;
  if (isLosslessNumber(value)) return value.value;
  if (typeof value === "bigint") return String(value);
  if (Array.isArray(value)) return "[" + value.map(encode).join(",") + "]";
  if (value !== null && typeof value === "object") {
    return "{" + Object.entries(value).filter(([, item]) => item !== undefined)
      .map(([key, item]) => encode(key) + ":" + encode(item)).join(",") + "}";
  }
  const text = JSON.stringify(value);
  need(text !== undefined, "invalid", "undefined is not a control value");
  return text.replace(/[<>&\u2028\u2029]/gu, character => "\\u" + character.charCodeAt(0).toString(16).padStart(4, "0"));
}
export function field(text, name) {
  let position = 0, found;
  const whitespace = () => { while (/\s/u.test(text[position] ?? "") && position < text.length) position++; };
  const quoted = () => {
    need(text[position++] === "\"", "corrupt", "stored JSON key is invalid");
    while (position < text.length) {
      const character = text[position++];
      if (character === "\\") position++;
      else if (character === "\"") return;
    }
    throw new Fault("corrupt", "stored JSON string is incomplete");
  };
  whitespace();
  need(text[position++] === "{", "corrupt", "stored JSON object is invalid");
  for (;;) {
    whitespace();
    if (text[position] === "}") break;
    const start = position;
    quoted();
    const key = JSON.parse(text.slice(start, position));
    whitespace();
    need(text[position++] === ":", "corrupt", "stored JSON field is invalid");
    whitespace();
    const valueStart = position;
    let depth = 0;
    while (position < text.length) {
      const character = text[position];
      if (character === "\"") { quoted(); continue; }
      if (depth === 0 && (character === "," || character === "}")) break;
      if (character === "[" || character === "{") depth++;
      else if (character === "]" || character === "}") depth--;
      position++;
    }
    if (key === name) found = text.slice(valueStart, position).trim();
    if (text[position] === "}") break;
    need(text[position++] === ",", "corrupt", "stored JSON object is incomplete");
  }
  return found;
}
const fields = name => {
  const definition = schema.$defs[name];
  const variants = definition.oneOf ?? definition.anyOf ?? [definition];
  return [...new Set(variants.flatMap(variant => Object.keys(variant.properties)))];
};
export function wire(name, value) {
  return encode(Object.fromEntries(fields(name).filter(key => value[key] !== undefined).map(key => [key, value[key]])));
}
export async function digest(text) {
  return [...new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)))]
    .map(value => value.toString(16).padStart(2, "0")).join("");
}
export async function id(...parts) { return (await digest(encode(parts))).slice(0, 40); }
export async function capability(kind, ...parts) { return kind + ":" + await id(...parts); }
export function identifiers(values) {
  need(Array.isArray(values) && values.length <= 4096 && values.every(value => typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(value)),
    "invalid", "capability catalogue is invalid or oversized");
  return [...new Set(values)].sort();
}
function binding(value) {
  if (isLosslessNumber(value) || typeof value === "bigint") return String(value);
  if (typeof value === "boolean") return value ? 1 : 0;
  need(value === null || typeof value === "string" || (typeof value === "number" && Number.isSafeInteger(value)), "invalid", "invalid SQL domain parameter");
  need(typeof value !== "string" || new TextEncoder().encode(value).byteLength <= 1048576,
    "overloaded", "D1 domain value exceeds its one-MiB reserve-safe bound");
  return value;
}
export function statement(db, sql, values = []) {
  need(values.length <= 100 && sql.length <= 100000, "overloaded", "D1 statement exceeds the bounded domain operation");
  return db.prepare(sql).bind(...values.map(binding));
}
export async function rows(db, sql, values = []) {
  const result = await statement(db, sql, values).all();
  need(result.success, "unavailable", "D1 read did not complete");
  return result.results;
}
export async function one(db, sql, values = [], optional = false) {
  const result = await rows(db, sql, values);
  need(optional || result.length !== 0, "not_found", "control fact not found");
  return result[0];
}
export async function initialize(db) {
  const exists = await one(db, "SELECT name FROM sqlite_master WHERE type='table' AND name='brain_schema'", [], true);
  if (exists) {
    const version = await one(db, "SELECT MAX(version) AS version FROM brain_schema");
    need(version.version === 13, "unsupported", "D1 control schema requires a verified offline migration");
  }
  const statements = sqlSchema.split(";").map(value => value.trim()).filter(Boolean);
  statements.push(
    "CREATE INDEX IF NOT EXISTS sessions_ready ON sessions(ready,tenant,ready_ms,id)",
    "CREATE TABLE IF NOT EXISTS d1_scopes(key TEXT PRIMARY KEY,version INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS d1_guards(id TEXT PRIMARY KEY,ok INTEGER NOT NULL CHECK(ok=1))",
    "CREATE TABLE IF NOT EXISTS d1_scope_conflicts(failed INTEGER NOT NULL CONSTRAINT brain_d1_scope_changed CHECK(failed=0))",
  );
  need(statements.length <= 100, "invalid", "initial schema exceeds the D1 atomic batch bound");
  await db.batch(statements.map(sql => db.prepare(sql)));
}

export const sessionColumns = `id,tenant,config,lifecycle,CAST(revision AS TEXT) AS revision,CAST(sequence AS TEXT) AS sequence,
owner,CAST(epoch AS TEXT) AS epoch,CAST(generation AS TEXT) AS generation,paused,created_ms,parent,budget`;
export const operationColumns = `id,session_id,run_id,step_key,kind,action,arguments_digest,status,recipient,CAST(revision AS TEXT) AS revision,
arguments,output_limit,deadline_ms,generation,CAST(result_sequence AS TEXT) AS result_sequence,result_digest,result_body,usage,claimant,
retry_of,error,CAST(intent_sequence AS TEXT) AS intent_sequence,intent_digest,token_reserve,approval,receipt_hash,receipt_key`;
export const commandColumns = `id,session_id,status,accepted_ms,run_id,CAST(sequence AS TEXT) AS sequence,error`;
export const runColumns = `r.id,r.session_id,r.command_id,r.status,CAST(r.step AS TEXT) AS step,r.checkpoint,r.decision,r.wait_kind,r.wait_id,
r.error,c.payload,CAST(c.sequence AS TEXT) AS sequence,CAST(r.completion_sequence AS TEXT) AS completion_sequence,r.completion_digest,r.harness_process`;
export const connectionColumns = "id,session_id,template,CAST(generation AS TEXT) AS generation,status,ownership,resource_id,claim_subject";
export const interactionColumns = "id,session_id,CAST(revision AS TEXT) AS revision,status,request,answer,answer_key";
export const documentColumns = "key,CAST(revision AS TEXT) AS revision,value,expires_ms";
export const scheduleColumns = "id,session_id,due_ms,status,request";
export const fileColumns = "name,digest,size,media_type,provenance";

export function session(row) {
  return { View: { id: row.id, lifecycle: row.lifecycle, revision: row.revision, sequence: row.sequence,
    config: raw(row.config), created_ms: row.created_ms, owner_epoch: row.epoch, paused: row.paused !== 0,
    ...(row.parent ? { parent: row.parent } : {}) }, Tenant: row.tenant, Owner: { Node: row.owner, Epoch: integer(row.epoch) },
    Generation: integer(row.generation), Budget: row.budget };
}
export function operation(row) {
  const result = row.result_body ? field(row.result_body, "result") : undefined;
  return { View: { id: row.id, session_id: row.session_id, run_id: row.run_id, step_key: row.step_key, kind: row.kind,
    action: row.action, arguments_digest: row.arguments_digest, status: row.status, recipient: row.recipient, revision: row.revision,
    result_available: row.result_body !== "", ...(result ? { result: raw(result) } : {}), ...(row.usage ? { usage: raw(row.usage) } : {}),
    ...(row.error ? { error: row.error } : {}), ...(row.retry_of ? { retry_of: row.retry_of } : {}) },
    ...(row.arguments ? { Arguments: raw(row.arguments) } : {}), OutputLimit: row.output_limit, Deadline: row.deadline_ms, Generation: row.generation,
    ResultSequence: integer(row.result_sequence), ResultDigest: row.result_digest, Claimant: row.claimant,
    IntentSequence: integer(row.intent_sequence), IntentDigest: row.intent_digest, TokenReserve: row.token_reserve };
}
export function command(row) {
  return { id: row.id, session_id: row.session_id, status: row.status, accepted_ms: row.accepted_ms,
    ...(row.run_id ? { run_id: row.run_id } : {}), ...(row.status === "applied" && big(row.sequence) > 0n ? { sequence: row.sequence } : {}),
    ...(row.error ? { error: raw(row.error) } : {}) };
}
export function run(row) {
  return { View: { id: row.id, session_id: row.session_id, command_id: row.command_id, status: row.status, step: integer(row.step),
    ...(big(row.completion_sequence) > 0n ? { completion: { session_id: row.session_id, sequence: row.completion_sequence, digest: row.completion_digest } } : {}),
    ...(row.wait_kind ? { wait: { kind: row.wait_kind, id: row.wait_id } } : {}), ...(row.error ? { error: raw(row.error) } : {}) },
    Input: raw(row.payload), InputSequence: integer(row.sequence), Checkpoint: raw(row.checkpoint), Decision: raw(row.decision),
    HarnessProcess: row.harness_process };
}
export function connection(row) {
  return { id: row.id, session_id: row.session_id, template: row.template, generation: row.generation, status: row.status,
    ownership: row.ownership, ...(row.resource_id ? { resource_id: row.resource_id } : {}),
    ...(row.claim_subject ? { claim_subject: row.claim_subject } : {}) };
}
export function interaction(row) {
  return { id: row.id, session_id: row.session_id, revision: row.revision, status: row.status,
    request: raw(row.request), ...(row.answer ? { answer: raw(row.answer) } : {}) };
}
export function document(row) {
  return { key: row.key, revision: row.revision, value: raw(row.value), ...(row.expires_ms ? { expires_ms: row.expires_ms } : {}) };
}
export function schedule(row) { return { id: row.id, session_id: row.session_id, due_ms: row.due_ms, status: row.status, request: raw(row.request) }; }

export function invalidParticipantAncestor(expiryOnly = false) {
  return `SELECT 1 FROM participant_ancestors l LEFT JOIN participants a ON a.id=l.ancestor
WHERE l.descendant=p.id AND (a.id IS NULL OR a.session_id<>p.session_id OR a.expires_ms<=${clock}${expiryOnly ? "" : " OR a.revoked<>0"})`;
}

export async function authoritySubject(m) {
  const authority = m.authority;
  if (!authority) return "";
  if (!authority.Participant) return authority.Subject;
  return (await m.one("SELECT subject FROM participants WHERE id=? AND session_id=?", [authority.Participant, authority.Session])).subject;
}

// Only named domain handlers create these plans. The caller cannot supply SQL.
// Every read/write scope is compared inside the same D1 batch as its changes.
export class Mutation {
  constructor(db) {
    this.db = db.withSession("first-primary");
    this.authority = db.authority;
    this.scopes = new Map();
    this.guards = [];
    this.writes = [];
    this.reads = db.planReads ?? { count: 0 };
  }
  async scope(key) {
    if (this.scopes.has(key)) return;
    need(this.scopes.size < 16, "overloaded", "D1 operation touches too many atomic scopes");
    const value = await this.one("SELECT CAST(version AS TEXT) AS version FROM d1_scopes WHERE key=?", [key], true);
    this.scopes.set(key, value?.version);
  }
  async rows(sql, values = []) {
    need(++this.reads.count <= 96, "overloaded", "D1 operation exceeds its bounded read plan");
    return rows(this.db, sql, values);
  }
  async one(sql, values = [], optional = false) {
    const result = await this.rows(sql, values);
    need(optional || result.length !== 0, "not_found", "control fact not found");
    return result[0];
  }
  async session(id, live = false) {
    await this.scope("session:" + id);
    const value = await this.one("SELECT " + sessionColumns + " FROM sessions WHERE id=?", [id]);
    need(!live || value.lifecycle === "open", "conflict", "session is not open");
    if (live) {
      this.guard("EXISTS(SELECT 1 FROM sessions WHERE id=? AND lifecycle='open')", [id]);
      await this.lineage(id);
    }
    return value;
  }
  async lineage(id, dispatch = false) {
    const predicate = `NOT EXISTS(SELECT 1 FROM budget_members m JOIN budgets b ON b.id=m.budget_id
WHERE m.session_id=? AND ((m.budget_id<>m.session_id AND m.generation<>b.cancel_generation) OR (?=1 AND b.paused<>0)))`;
    const args = [id, dispatch];
    const valid = await this.one("SELECT CASE WHEN " + predicate + " THEN 1 ELSE 0 END AS valid", args);
    need(valid.valid === 1, "conflict", "an enclosing session cancelled or paused this delegated authority");
    this.guard(predicate, args);
  }
  async owner(id, owner, dispatch = false) {
    const value = await this.session(id);
    need(value.owner === owner.Node && value.epoch === String(owner.Epoch) && value.lifecycle !== "deleted",
      "conflict", "session ownership changed");
    const predicate = `EXISTS(SELECT 1 FROM sessions s JOIN nodes n ON n.id=s.owner WHERE s.id=? AND s.owner=?
AND s.epoch=? AND s.lifecycle<>'deleted' AND n.lease>${clock} AND n.draining=0${dispatch ? " AND s.lifecycle='open' AND s.paused=0" : ""})`;
    const args = [id, owner.Node, owner.Epoch];
    const valid = await this.one("SELECT CASE WHEN " + predicate + " THEN 1 ELSE 0 END AS valid", args);
    need(valid.valid === 1, "conflict", "session has no live dispatch authority");
    this.guard(predicate, args);
    if (dispatch) {
      await this.lineage(id, true);
      await this.unfrozen(id);
    }
    return value;
  }
  async unfrozen(id, allocation = false) {
    const predicate = `NOT EXISTS(SELECT 1 FROM ownership_transfers WHERE session_id=? AND ${
      allocation ? "phase='verifying'" : "phase IN ('draining','verifying')"})`;
    const valid = await this.one("SELECT CASE WHEN " + predicate + " THEN 1 ELSE 0 END AS valid", [id]);
    need(valid.valid === 1, "conflict", "session is frozen for a planned ownership transfer");
    this.guard(predicate, [id]);
  }
  guard(predicate, values = []) { this.guards.push({ predicate, values }); }
  change(sql, values = [], exact = false) { this.writes.push({ sql, values, exact }); }
  async commit(reads = []) {
    let authorityEnd;
    if (this.authority) {
      const authority = this.authority;
      const conditions = [], expiry = [], values = [], expiryValues = [];
      if (authority.Expires !== undefined) {
        conditions.push(`?>${clock}`);
        expiry.push(`?>${clock}`);
        values.push(authority.Expires);
        expiryValues.push(authority.Expires);
      }
      if (authority.Session) {
        await this.scope("session:" + authority.Session);
        conditions.push("EXISTS(SELECT 1 FROM sessions WHERE id=? AND tenant=? AND lifecycle<>'deleted')");
        values.push(authority.Session, authority.Tenant);
      }
      if (authority.Participant) {
        conditions.push(`EXISTS(SELECT 1 FROM participants p WHERE p.id=? AND p.session_id=? AND
p.revoked=0 AND p.expires_ms>${clock} AND NOT EXISTS(${invalidParticipantAncestor()}))`);
        values.push(authority.Participant, authority.Session);
        expiry.push(`EXISTS(SELECT 1 FROM participants p WHERE p.id=? AND p.session_id=? AND
p.expires_ms>${clock} AND NOT EXISTS(${invalidParticipantAncestor(true)}))`);
        expiryValues.push(authority.Participant, authority.Session);
      }
      const predicate = conditions.join(" AND ");
      const valid = await this.one("SELECT CASE WHEN " + predicate + " THEN 1 ELSE 0 END AS valid", values);
      need(valid.valid === 1, "unauthorized", "participant authority was revoked or expired before commit");
      this.guard(predicate, values);
      authorityEnd = { predicate: expiry.join(" AND "), values: expiryValues };
    }
    if (this.writes.length === 0 && this.guards.length === 0) {
      return Promise.all(reads.map(({ sql, values = [] }) => rows(this.db, sql, values)));
    }
    const batch = [];
    let guardID = 0;
    const changed = () => batch.push(statement(this.db, "INSERT INTO d1_guards VALUES(?,changes())", ["changed:" + guardID++]));
    for (const [key, version] of this.scopes) {
      if (version === undefined) batch.push(statement(this.db, "INSERT INTO d1_scopes VALUES(?,1) ON CONFLICT DO NOTHING", [key]));
      else batch.push(statement(this.db, "UPDATE d1_scopes SET version=version+1 WHERE key=? AND version=? AND version<9223372036854775807", [key, version]));
      // A named constraint proves this entire batch rolled back before any permit.
      batch.push(this.db.prepare("INSERT INTO d1_scope_conflicts SELECT 1 WHERE changes()<>1"));
    }
    for (const { predicate, values } of this.guards) {
      batch.push(statement(this.db, "INSERT INTO d1_guards SELECT ?,CASE WHEN " + predicate + " THEN 1 ELSE 0 END", ["condition:" + guardID++, ...values]));
    }
    for (const { sql, values, exact } of this.writes) {
      batch.push(statement(this.db, sql, values));
      if (exact) changed();
    }
    if (authorityEnd) {
      batch.push(statement(this.db, "INSERT INTO d1_guards SELECT ?,CASE WHEN " + authorityEnd.predicate + " THEN 1 ELSE 0 END",
        ["authority:end", ...authorityEnd.values]));
    }
    batch.push(this.db.prepare("DELETE FROM d1_guards"));
    const firstRead = batch.length;
    for (const { sql, values = [] } of reads) batch.push(statement(this.db, sql, values));
    need(batch.length <= 100, "overloaded", "D1 operation exceeds the atomic batch statement bound");
    let result;
    try {
      result = await this.db.batch(batch);
    } catch (error) {
      const message = String(error?.cause?.message ?? error?.message ?? "");
      if (/\bCHECK constraint failed: brain_d1_scope_changed\b/u.test(message)) throw new ScopeChanged();
      throw error;
    }
    need(result.every(value => value.success), "uncertain", "D1 mutation result is uncertain");
    return result.slice(firstRead).map(value => value.results);
  }
}

export async function result(mutation, sql, values, convert = value => value) {
  const [valuesAfter] = await mutation.commit([{ sql, values }]);
  need(valuesAfter.length !== 0, "not_found", "control result was not found");
  return convert(valuesAfter[0]);
}
