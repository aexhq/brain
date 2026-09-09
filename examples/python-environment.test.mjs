import assert from "node:assert/strict";
import { cp, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { pythonEnvironment } from "./python-environment.mjs";

async function fixture(t, setup = "prepare", module = "run") {
  const directory = await mkdtemp(join(tmpdir(), "brain-python-project-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await cp(new URL("../tests/fixtures/python-project/", import.meta.url), directory, { recursive: true });
  const env = pythonEnvironment({ uv: process.env.BRAIN_TEST_UV ?? "uv", projects: { example: { directory, setup, module } } });
  let sequence = 0;
  const send = async (request, session_id = "ses_one") => (await env.handle({ contract: "environment/v1",
    operation: { sequence: ++sequence, session_id, environment: "python", request } })).receipt;
  const attach = (session) => send({ type: "setup", configuration: {} }, session);
  const execute = (input, session) => send({ type: "execute", implementation: { type: "python_project", name: "example" }, input, deadline_ms: 30_000 }, session);
  await attach();
  return { directory, send, attach, execute };
}

test("Python imports follow locked preparation; concurrent first use shares setup across sessions", { timeout: 60_000 }, async (t) => {
  const f = await fixture(t);
  await f.attach("ses_two");
  const results = await Promise.all([f.execute("one"), f.execute("two", "ses_two")]);
  assert.deepEqual(results.map(({ output }) => output), [
    { echo: "one", dependency: "1.17.0" }, { echo: "two", dependency: "1.17.0" },
  ]);
  await f.send({ type: "teardown" });
  await f.attach();
  assert.equal((await f.execute("again")).type, "result");
  assert.equal(await readFile(join(f.directory, ".venv/prepared"), "utf8"), "1.17.0");
});

test("failed setup prevents imports and is not silently retried", { timeout: 60_000 }, async (t) => {
  const f = await fixture(t, "fail_setup");
  for (const result of await Promise.all([f.execute("one"), f.execute("two")])) {
    assert.equal(result.code, "preparation_failed");
    assert.match(result.message, /fixture setup failure/u);
  }
  assert.equal((await f.execute("explicit new invocation")).code, "preparation_failed");
  assert.equal(await readFile(join(f.directory, ".venv/setup_attempts"), "utf8"), "attempt\n");
  await assert.rejects(readFile(join(f.directory, ".venv/prepared")), { code: "ENOENT" });
});

test("standard packaging needs no custom hook and unknown placement fails explicitly", { timeout: 60_000 }, async (t) => {
  const f = await fixture(t, null, "plain");
  assert.deepEqual(await f.execute({}), { type: "result", output: { dependency: "1.17.0" } });
  assert.equal((await f.send({ type: "execute", implementation: { type: "python_project", name: "unknown" }, input: {} })).code, "unsupported");
  await assert.rejects(readFile(join(f.directory, ".venv/prepared")), { code: "ENOENT" });
});
