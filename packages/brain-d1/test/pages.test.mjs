import test from "node:test";
import assert from "node:assert/strict";
import { mkdir, rm } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { startD1 } from "../../../tests/fixtures/d1.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

test("actual D1 byte-bounded pages do not hide ready work behind waiting runs", async t => {
  const directory = path.join(root, ".test-output", "d1-pages-" + randomUUID());
  await mkdir(directory, { recursive: true });
  const f = await startD1(directory);
  t.after(async () => { await f.dispose(); await rm(directory, { recursive: true, force: true }); });
  async function call(method, args = []) {
    const response = await f.rpc(method, args);
    assert.equal(response.status, 200, `${method}: ${response.text}`);
    assert(Buffer.byteLength(response.text) < 2 * 1024 * 1024, "control page exceeded its byte bound");
    return response.data.value;
  }
  await call("Initialize");
  const session = await call("Create", ["tenant", { request_id: "pages", config: { limits: {
    calls: 1000, concurrent: 256, input_bytes: 65536, output_bytes: 65536, retained_bytes: 16777216, pending_inputs: 256,
  } } }, "", "", []]);
  const id = session.View.id;
  const payload = JSON.stringify({ kind: "message", text: "x".repeat(32768) });
  for (let start = 0; start < 161; start += 16) {
    const batch = [];
    for (let index = start; index < Math.min(start + 16, 161); index++) {
      const run = index === 160 ? "zz-ready" : "run-" + String(index).padStart(3, "0");
      batch.push(f.database.prepare(`INSERT INTO commands(id,session_id,request_key,digest,payload,source,status,accepted_ms,kind)
VALUES(?,?,?,'fixture',?,'fixture','applied',1,'message')`).bind(run, id, run, payload));
      batch.push(f.database.prepare("INSERT INTO runs(id,session_id,command_id,status,wait_kind,updated_ms) VALUES(?,?,?,?,?,1)")
        .bind(run, id, run, index === 160 ? "queued" : "waiting", index === 160 ? "" : "input"));
    }
    await f.database.batch(batch);
  }
  let after = "", count = 0, pages = 0;
  for (;;) {
    const page = await call("Runs", [id, 128, after]);
    if (page.length === 0) break;
    count += page.length;
    pages++;
    const next = page.at(-1).View.id;
    assert(next > after);
    after = next;
  }
  assert.equal(count, 161);
  assert(pages >= 3);
  assert.deepEqual((await call("RunnableRuns", [id, 64])).map(value => value.View.id), ["zz-ready"]);
  assert.equal((await call("ChildState", [id])).Pending.toString(), "161");
});
