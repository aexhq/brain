import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

async function runner(t, plugin) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'brain-pi-plugins-'));
  const child = spawn(process.execPath, [fileURLToPath(new URL('../dist/main.js', import.meta.url))], { cwd: dir, stdio: ['pipe', 'pipe', 'pipe'] });
  let diagnostics = '';
  child.stderr.on('data', bytes => { diagnostics += bytes; });
  t.after(async () => { if (child.exitCode === null && !child.signalCode) { const done = once(child, 'exit'); child.kill(); await done; } await rm(dir, { recursive: true, force: true }); });
  let seq = 0;
  const pending = new Map();
  readline.createInterface({ input: child.stdout }).on('line', line => {
    const frame = JSON.parse(line), entry = pending.get(frame.id);
    assert.ok(entry, line);
    pending.delete(frame.id); clearTimeout(entry.timer);
    if (frame.error) entry.reject(new Error(JSON.stringify(frame.error) + diagnostics)); else entry.resolve(frame.result);
  });
  const request = (method, params) => new Promise((resolve, reject) => {
    const id = String(++seq), timer = setTimeout(() => reject(new Error('Plugin runner timed out: ' + diagnostics)), 30000);
    pending.set(id, { resolve, reject, timer });
    child.stdin.write(JSON.stringify({ id, attachment: 'test', method, params }) + '\n');
  });
  const opened = await request('open', { session_id: 'test', dir: path.join(dir, 'native'), plugins: [plugin], config: { model: 'openai/gpt-4.1-mini', tools: [] }, tools: [], provider: { api_key: 'plugin-test-key' } });
  return { opened, request };
}

for (const [name, tool] of [['web-access', 'fetch_content'], ['subagents', 'subagent'], ['mcp', undefined]]) {
  test(`installed ${name} plugin opens headlessly without inference or ambient tools`, { timeout: 45000 }, async t => {
    const client = await runner(t, { name });
    if (tool) assert.ok(client.opened.tools.includes(tool));
    assert.ok(!client.opened.tools.some(name => ['bash', 'read', 'write', 'edit', 'execute'].includes(name)));
    assert.deepEqual(client.opened.tools, (client.opened.native_tools ?? []).map(tool => tool.name).sort());
    await client.request('detach');
  });
}

test('plugin tool selection rejects a missing tool instead of expanding the catalog', { timeout: 45000 }, async t => {
  await assert.rejects(runner(t, { name: 'web-access', tools: ['not_a_tool'] }), /failed to load/);
});
