import { big, decode, encode, field, need, rows, safe, wire } from "./domain.mjs";
import { isLosslessNumber } from "lossless-json";
import { outputReservation } from "./receipts.mjs";

const columns = `b.id,b.calls,b.active,b.reserved,b.retained,CAST(b.tokens AS TEXT) AS tokens,
CAST(b.reserved_tokens AS TEXT) AS reserved_tokens,b.incomplete_usages,b.max_calls,b.max_active,b.max_bytes,b.max_tokens,
b.paused,CAST(b.cancel_generation AS TEXT) AS generation,CAST(m.generation AS TEXT) AS admitted`;
const query = "SELECT " + columns + " FROM budgets b JOIN budget_members m ON m.budget_id=b.id WHERE m.session_id=? ORDER BY b.id";
const byteLength = value => new TextEncoder().encode(value).length;
const maximumDepth = 8;

function canonical(value) {
  if (value !== null && typeof value === "object" && !Array.isArray(value) && !isLosslessNumber(value)) {
    return encode(Object.fromEntries(Object.keys(value).filter(key => value[key] !== undefined).sort().map(key => [key, decode(canonical(value[key]))])));
  }
  if (Array.isArray(value)) return "[" + value.map(canonical).join(",") + "]";
  return encode(value);
}
export const equal = (a, b) => canonical(a) === canonical(b);
const subset = (child = [], parent = []) => child.every(value => parent.includes(value));

export function validateDelegation(parent, child) {
  const p = parent.limits, c = child.limits;
  need(subset(child.grants, parent.grants) && subset(child.skills, parent.skills) &&
    ["calls", "concurrent", "input_bytes", "output_bytes", "retained_bytes", "pending_inputs"].every(key => big(c[key]) <= big(p[key])) &&
    (p.tokens === undefined || (c.tokens !== undefined && big(c.tokens) <= big(p.tokens))) &&
    (p.deadline_ms === undefined || (c.deadline_ms !== undefined && big(c.deadline_ms) <= big(p.deadline_ms))),
  "forbidden", "delegation cannot amplify grants, skills or resource limits");
  need(equal(parent.model, child.model), "forbidden", "delegation cannot replace admitted model authority");
  const admitted = selection => [parent.harness, ...(parent.harness_transitions ?? [])].some(original => equal(original, selection));
  need(admitted(child.harness) && (child.harness_transitions ?? []).every(admitted),
    "forbidden", "delegation cannot add harness transitions");
  for (const connection of child.connections ?? []) {
    need((parent.connections ?? []).some(original => equal(original, connection)), "forbidden", "delegation cannot add connections or secrets");
  }
  for (const action of child.actions ?? []) {
    const original = (parent.actions ?? []).find(value => value.name === action.name);
    need(original && big(action.max_output_bytes) <= big(original.max_output_bytes) && big(action.timeout_ms) <= big(original.timeout_ms) &&
      (!original.approval || action.approval) &&
      equal({ ...original, max_output_bytes: action.max_output_bytes, timeout_ms: action.timeout_ms, approval: action.approval }, action),
    "forbidden", "delegation cannot amplify action authority, schemas or placement");
  }
  for (const route of child.events ?? []) {
    const original = (parent.events ?? []).find(value => value.name === route.name && value.version === route.version);
    need(original && (!(original.sources ?? []).length || ((route.sources ?? []).length && subset(route.sources, original.sources))) &&
      equal({ ...original, sources: route.sources }, route), "forbidden", "delegation cannot amplify event routing authority");
  }
}

export async function createBudget(m, identifier, request, parent) {
  let depth = 0;
  if (parent) {
    const ancestry = await m.one("SELECT MAX(depth)+1 AS depth FROM budget_members WHERE session_id=?", [parent]);
    depth = ancestry.depth;
    need(Number.isInteger(depth) && depth <= maximumDepth, "forbidden", "delegation depth limit reached");
  }
  await m.scope("budget:" + identifier);
  m.change("INSERT INTO budgets(id,max_calls,max_active,max_bytes,max_tokens) VALUES(?,?,?,?,?)",
    [identifier, request.config.limits.calls, request.config.limits.concurrent, request.config.limits.retained_bytes, request.config.limits.tokens ?? -1]);
  m.change("INSERT INTO budget_members VALUES(?,?,?,1)", [identifier, identifier, depth]);
  if (parent) m.change(`INSERT INTO budget_members SELECT ?,m.budget_id,m.depth,b.cancel_generation
FROM budget_members m JOIN budgets b ON b.id=m.budget_id WHERE m.session_id=?`, [identifier, parent]);
  else m.change("INSERT INTO tree_authority VALUES(?)", [identifier]);
}

export function tokenReservation(configText, argumentsText) {
  const config = decode(configText), model = config.model, options = model.native ?? {};
  const limits = ["max_tokens", "max_completion_tokens", "max_output_tokens"].filter(key => options[key] !== undefined);
  need(limits.length <= 1, "invalid", "one bounded integral model output-token limit is required");
  const output = limits.length ? safe(options[limits[0]], 1, 1048576) : 4096;
  if (config.limits.tokens !== undefined) {
    need(["previous_response_id", "conversation", "container"].every(key => options[key] === undefined),
      "unsupported", "token-bounded sessions require explicit local model context");
    const request = decode(argumentsText);
    need((request.messages ?? []).every(message => message.content === null || typeof message.content === "string"),
      "unsupported", "token-bounded sessions currently require text model messages");
  }
  return byteLength(argumentsText) + byteLength(field(configText, "model") ?? wire("ModelSelection", model)) + output + 1024;
}

async function pools(m, identifier) {
  const values = await m.rows(query, [identifier]);
  need(values.length > 0 && values.length <= maximumDepth + 1, "corrupt", "session budget ancestry is invalid");
  for (const value of values) await m.scope("budget:" + value.id);
  return values;
}

export async function reserveBudgets(m, identifier, bytes, tokens) {
  for (const pool of await pools(m, identifier)) {
    need(pool.calls < pool.max_calls, "forbidden", "session or ancestor call limit reached");
    need(pool.active < pool.max_active && pool.retained + pool.reserved + bytes <= pool.max_bytes,
      "overloaded", "session or ancestor concurrency or output reserve is exhausted");
    if (pool.max_tokens >= 0 && tokens > 0 && big(pool.tokens) + big(pool.reserved_tokens) + BigInt(tokens) > BigInt(pool.max_tokens)) {
      need(false, big(pool.reserved_tokens) === 0n ? "forbidden" : "overloaded",
        big(pool.reserved_tokens) === 0n ? "model request does not fit the remaining token budget" : "token budget is held by in-flight or incomplete usage");
    }
    m.change(`UPDATE budgets SET calls=calls+1,active=active+1,reserved=reserved+?,reserved_tokens=reserved_tokens+?
WHERE id=? AND calls<max_calls AND active<max_active AND retained+reserved+?<=max_bytes
AND (?=0 OR max_tokens<0 OR tokens+reserved_tokens+?<=max_tokens)`, [bytes, tokens, pool.id, bytes, tokens, tokens], true);
  }
}

export async function settleBudgets(m, prior, receipt, bodyBytes, notSent = false, bound = false) {
  let reported = 0n, complete = true;
  if (!notSent && prior.kind === "model") {
    const input = receipt.usage?.input_tokens, output = receipt.usage?.output_tokens;
    complete = input !== undefined && output !== undefined;
    if (big(input ?? 0) > (1n << 40n) || big(output ?? 0) > (1n << 40n)) complete = false;
    else reported = big(input ?? 0) + big(output ?? 0);
  }
  const release = complete ? prior.token_reserve : 0, reserve = bound ? outputReservation(prior.output_limit) : 0;
  for (const pool of await pools(m, prior.session_id)) {
    m.change(`UPDATE budgets SET active=active-1,reserved=reserved-?,retained=retained+?,
tokens=tokens+?,reserved_tokens=reserved_tokens-?,incomplete_usages=incomplete_usages+?
WHERE id=? AND active>0 AND reserved>=? AND reserved_tokens>=?`,
    [reserve, bodyBytes, reported, release, complete ? 0 : 1, pool.id, reserve, release], true);
  }
}

export async function releaseOutputBudgets(m, operation) {
  const reserve = outputReservation(operation.output_limit);
  for (const pool of await pools(m, operation.session_id)) {
    m.change("UPDATE budgets SET reserved=reserved-? WHERE id=? AND reserved>=?", [reserve, pool.id, reserve], true);
  }
}

export async function refineTokenBudgets(m, prior, difference, complete) {
  const reserve = complete ? prior.token_reserve : 0, incomplete = complete ? 1 : 0;
  for (const pool of await pools(m, prior.session_id)) {
    m.change(`UPDATE budgets SET tokens=tokens+?,reserved_tokens=reserved_tokens-?,incomplete_usages=incomplete_usages-?
WHERE id=? AND reserved_tokens>=? AND incomplete_usages>=?`,
    [difference, reserve, incomplete, pool.id, reserve, incomplete], true);
  }
}

export const budgetHandlers = {
  async Budget(db, [identifier]) {
    const values = await rows(db, query, [identifier]);
    const own = values.find(value => value.id === identifier);
    need(own && values.length <= maximumDepth + 1, "corrupt", "session has no independent budget pool");
    const value = {
      session_id: identifier, calls: own.calls, active_operations: own.active, retained_bytes: own.retained,
      reserved_output_bytes: own.reserved, reported_tokens: own.tokens, reserved_tokens: own.reserved_tokens,
      incomplete_usages: own.incomplete_usages, token_usage_pending: false, remaining_calls: Number.MAX_SAFE_INTEGER,
      available_operations: Number.MAX_SAFE_INTEGER, available_bytes: Number.MAX_SAFE_INTEGER,
    };
    for (const pool of values) {
      value.token_usage_pending ||= big(pool.reserved_tokens) > 0n;
      value.remaining_calls = Math.min(value.remaining_calls, Math.max(0, pool.max_calls - pool.calls));
      value.available_operations = Math.min(value.available_operations, Math.max(0, pool.max_active - pool.active));
      if (pool.paused !== 0 || (pool.id !== identifier && pool.generation !== pool.admitted)) value.available_operations = 0;
      value.available_bytes = Math.min(value.available_bytes, Math.max(0, pool.max_bytes - pool.retained - pool.reserved));
      if (pool.max_tokens >= 0) {
        let available = BigInt(pool.max_tokens) - big(pool.tokens) - big(pool.reserved_tokens);
        if (available < 0n) available = 0n;
        if (value.available_tokens !== undefined && BigInt(value.available_tokens) < available) available = BigInt(value.available_tokens);
        value.available_tokens = String(available);
      }
    }
    return value;
  },
};
