import assert from 'node:assert/strict';
import { test } from 'node:test';
import { main } from '../dist/cli.mjs';

test('CLI help and invalid arguments do not start a client or exit the host process', async () => {
  const output = [];
  const client = { dispose() { assert.fail('help must not initialize a client'); } };
  await main(['--help'], { client, output: text => output.push(text) });
  assert.match(output.join('\n'), /upload-tool/);
  assert.match(output.join('\n'), /create/);
  await assert.rejects(main(['create', '--id', 'one'], { client }), /model/);
  await assert.rejects(main(['unknown'], { client }), /unknown command/);
});

test('CLI dispatches SDK actions asynchronously and disposes on success and failure', async () => {
  const calls = [], output = [];
  const client = {
    async send(id, input) { calls.push({ id, input }); return { id: 'event' }; },
    async get() { throw new Error('server unavailable'); },
    async dispose() { calls.push('disposed'); },
  };
  await main(['send', 'customer/session', 'hello', 'world'], { client, output: value => output.push(value) });
  assert.deepEqual(calls, [{ id: 'customer/session', input: { type: 'user.message', text: 'hello world' } }, 'disposed']);
  assert.deepEqual(output, [{ id: 'event' }]);
  await assert.rejects(main(['get', 'one'], { client }), /server unavailable/);
  assert.equal(calls.at(-1), 'disposed');
  assert.equal(calls.filter(value => value === 'disposed').length, 2);
});
