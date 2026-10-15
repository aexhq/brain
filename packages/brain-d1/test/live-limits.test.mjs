import test from "node:test";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";

test("live Cloudflare D1 enforces its row and bound-parameter limits", async t => {
  const account = process.env.BRAIN_D1_ACCOUNT_ID;
  const token = process.env.CLOUDFLARE_API_TOKEN;
  assert.match(account ?? "", /^[a-f0-9]{32}$/u, "BRAIN_D1_ACCOUNT_ID is required; a missing live credential is not a passing gate");
  assert(token, "CLOUDFLARE_API_TOKEN is required; no interactive login or credential fallback");
  const base = `https://api.cloudflare.com/client/v4/accounts/${account}/d1/database`;
  async function request(method, suffix, body) {
    const response = await fetch(base + suffix, { method, redirect: "error", signal: AbortSignal.timeout(60000),
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body) });
    const result = await response.json();
    return { status: response.status, ...result };
  }
  const created = await request("POST", "", { name: "brain-conformance-" + randomUUID() });
  assert.equal(created.success, true, `cannot create the disposable D1 database: HTTP ${created.status}`);
  const database = created.result.uuid;
  assert.match(database, /^[a-f0-9-]{36}$/u);
  t.after(async () => {
    const removed = await request("DELETE", "/" + database);
    assert.equal(removed.success, true, `disposable D1 cleanup failed for ${database}: HTTP ${removed.status}`);
  });
  const table = await request("POST", `/${database}/query`, { sql: "CREATE TABLE limit_probe(value TEXT)" });
  assert.equal(table.success, true);
  const large = await request("POST", `/${database}/query`, { sql: "INSERT INTO limit_probe VALUES(?)", params: ["x".repeat(2 * 1024 * 1024 + 1)] });
  assert.equal(large.success, false, "live D1 accepted a row exceeding its documented two-MiB ceiling");
  assert.match(JSON.stringify(large.errors), /too (?:big|large)|size|limit/iu);
  const parameters = await request("POST", `/${database}/query`, {
    sql: "SELECT " + Array.from({ length: 101 }, () => "?").join(","), params: Array(101).fill(1),
  });
  assert.equal(parameters.success, false, "live D1 accepted more than 100 bound parameters");
  assert.match(JSON.stringify(parameters.errors), /too many|bound|parameter|variable|limit/iu);
});
