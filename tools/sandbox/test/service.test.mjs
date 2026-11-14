import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { toolServer } from '../../http.mjs';
import { sandboxService } from '../src/service.mjs';
import { catalog } from '../src/catalog.mjs';
import { providersFromEnv } from '../src/providers.mjs';

test('create, cold service recovery, execute, ownership, expiration and cleanup over HTTP', async t => {
  let clock = Date.now(), created = 0, executions = 0, terminated = 0;
  const providers = { fixture: {
    async create() { created++; return 'remote-id'; },
    async inspect(id) { assert.equal(id, 'remote-id'); return { state: 'running' }; },
    async exec(id, command) { executions++; assert.equal(id, 'remote-id'); assert.match(command, /^python3 -c/); return JSON.stringify({ stdout: 'hello', stderr: '', exit_code: 0, timed_out: false, truncated: false }); },
    async terminate() { terminated++; },
  } };
  const opts = { providers, signingKey: 's'.repeat(32), now: () => clock };
  let invoke = sandboxService(opts);
  const server = toolServer({ token: 'k'.repeat(32), invoke: (...args) => invoke(...args) });
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  t.after(() => server.close());
  const url = `http://127.0.0.1:${server.address().port}`;
  async function call(path, args, session = 'one') {
    const res = await fetch(url + path, { method: 'POST', headers: { authorization: `Bearer ${'k'.repeat(32)}` }, body: JSON.stringify({ tenant_id: 'tenant', session_id: session, call_id: 'call', arguments: args }) });
    assert.equal(res.status, 200); return res.json();
  }
  assert.equal((await fetch(url + '/create', { method: 'POST' })).status, 401);
  assert.equal((await call('/create', { provider: 'missing' })).is_error, true);
  const sandbox = JSON.parse((await call('/create', { provider: 'fixture' })).content[0].text).sandbox;
  assert.equal(created, 1);
  invoke = sandboxService(opts); // Restore service with only its signing secret.
  assert.equal(JSON.parse((await call('/exec', { sandbox, command: 'echo hello' })).content[0].text).stdout, 'hello');
  assert.equal((await call('/exec', { sandbox, command: 'echo private' }, 'other')).is_error, true);
  assert.equal((await call('/exec', { sandbox: sandbox + 'x', command: 'echo private' })).is_error, true);
  assert.equal(executions, 1);
  assert.equal(JSON.parse((await call('/inspect', { sandbox })).content[0].text).state, 'running');
  clock += 301000;
  assert.equal((await call('/exec', { sandbox, command: 'echo late' })).is_error, true);
  assert.equal(JSON.parse((await call('/terminate', { sandbox })).content[0].text).terminated, true);
  assert.equal(terminated, 1);
});

test('ambiguous provider actions are never retried; cancelled creates are cleaned up', async () => {
  let calls = 0, cleanup = 0;
  const controller = new AbortController();
  const request = { tenant_id: 't', session_id: 's', call_id: 'c', arguments: { provider: 'fixture' } };
  let invoke = sandboxService({ signingKey: 's'.repeat(32), providers: { fixture: { async create() { calls++; throw new Error('secret must not leak'); } } } });
  const error = await invoke('/create', request, controller.signal);
  assert.equal(error.unknown_outcome, true); assert.equal(calls, 1); assert.doesNotMatch(JSON.stringify(error), /secret/);
  invoke = sandboxService({ signingKey: 's'.repeat(32), providers: { fixture: { async create() { controller.abort(); return 'id'; }, async terminate() { cleanup++; } } } });
  assert.equal((await invoke('/create', request, controller.signal)).unknown_outcome, true);
  assert.equal(cleanup, 1);
});

test('provider configuration and generated tool descriptors', () => {
  assert.throws(() => providersFromEnv({}), /at least one/);
  assert.throws(() => providersFromEnv({ MODAL_TOKEN_ID: 'id' }), /both/);
  const tools = catalog('http://localhost:8091', 'local', ['modal', 'e2b', 'daytona']);
  assert.equal(tools.length, 4);
  assert.deepEqual(tools[0].input_schema.properties.provider.enum, ['modal', 'e2b', 'daytona']);
  assert.ok(tools.every(t => t.tenants[0] === 'local' && t.token_env === 'SANDBOX_TOOL_TOKEN'));
});

test('remote execution helper bounds output and runtime and preserves exit status', () => {
  const run = spawnSync('python', ['test/execute_fixture.py'], { encoding: 'utf8', timeout: 10000, shell: process.platform === 'win32' });
  assert.equal(run.status, 0, run.stderr);
  const [normal, fail, large, slow] = JSON.parse(run.stdout);
  assert.match(normal.stdout, /hello/); assert.equal(fail.exit_code, 7);
  assert.equal(large.truncated, true); assert.equal(large.stdout.length, 1024);
  assert.equal(slow.timed_out, true);
});
