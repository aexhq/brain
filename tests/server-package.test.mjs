import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import http from 'node:http';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { after, before, test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
let directory, BrainServer, Model, Tool;
before(async () => {
  directory = await mkdtemp(path.join(tmpdir(), 'brainserver consumer '));
  await writeFile(path.join(directory, 'package.json'), JSON.stringify({ private: true, type: 'module' }));
  const release = JSON.parse(await readFile(path.join(root, 'dist/server/release.json'), 'utf8'));
  const packages = release.npmPackages.filter(p => p.name === 'brainserver' || p.name === `@aexhq/brainserver-${process.platform}-${process.arch}`);
  assert.equal(packages.length, 2, 'package the server and this platform before testing');
  const args = ['install', '--offline', '--ignore-scripts', '--no-audit', '--no-fund', ...packages.map(p => path.join(root, 'dist/server', p.file))];
  const command = process.platform === 'win32' ? process.execPath : 'npm';
  if (process.platform === 'win32') args.unshift(path.join(path.dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'));
  const installed = spawnSync(command, args, { cwd: directory, encoding: 'utf8', timeout: 240000, windowsHide: true });
  assert.ifError(installed.error);
  assert.equal(installed.status, 0, installed.stderr);
  const require = createRequire(path.join(directory, 'package.json'));
  ({ BrainServer, Model, Tool } = await import(pathToFileURL(require.resolve('brainserver')).href));
}, { timeout: 270000 });
after(async () => {
  if (directory) {
    assert.equal(path.dirname(directory), tmpdir());
    await rm(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});

test('installed TypeScript declarations expose the server and client without extra dependencies', async () => {
  await writeFile(path.join(directory, 'consumer.ts'), `import { BrainServer, Model, type Session } from 'brainserver';
const brain: BrainServer = await BrainServer.start({providers:{openai:{apiKey:'key'}}});
const session: Session = await brain.create({harness:'pi', model:Model('openai/gpt-4.1-mini'), tools:[]});
void session;
await brain.stop();
`);
  const checked = spawnSync(process.execPath, [path.join(root, 'sdks/typescript/node_modules/typescript/bin/tsc'), '--noEmit', '--strict', '--module', 'NodeNext', '--target', 'ES2023', 'consumer.ts'], { cwd: directory, encoding: 'utf8' });
  assert.equal(checked.status, 0, checked.stdout + checked.stderr);
});

async function provider(t) {
  const requests = [];
  const errors = [];
  const server = http.createServer(async (req, res) => {
    try {
      assert.equal(req.url, '/v1/chat/completions');
      assert.equal(req.headers.authorization, 'Bearer package-test-provider');
      let body = '';
      for await (const chunk of req) body += chunk;
      const input = JSON.parse(body);
      requests.push(input);
      const last = input.messages.at(-1);
      const call = last.role === 'user' && input.tools?.length;
      res.writeHead(200, { 'Content-Type': 'text/event-stream' });
      const chunk = (delta, finish_reason = null) => res.write(`data: ${JSON.stringify({
        id: 'package-test', object: 'chat.completion.chunk', created: 1, model: 'gpt-4.1-mini',
        choices: [{ index: 0, delta, finish_reason }],
      })}\n\n`);
      chunk({ role: 'assistant' });
      if (call) chunk({ tool_calls: [{ index: 0, id: 'call-package', type: 'function', function: { name: input.tools[0].function.name, arguments: '{"text":"hello"}' } }] });
      else chunk({ content: 'hello from packaged Pi' });
      chunk({}, call ? 'tool_calls' : 'stop');
      res.end('data: [DONE]\n\n');
    } catch (error) { errors.push(error); res.destroy(); }
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(async () => {
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
    assert.deepEqual(errors, []);
  });
  return { requests, options: {
    dataDir: path.join(directory, t.name),
    providers: { openai: { apiKey: 'package-test-provider', baseUrl: `http://127.0.0.1:${server.address().port}/v1`, api: 'openai-completions' } },
  } };
}
async function turn(brain, session) {
  const input = await brain.send(session.id, { type: 'user.message', text: 'hello' });
  const events = [];
  for await (const event of brain.observe(session.id, { signal: AbortSignal.timeout(30000) })) {
    if (event.input_id !== input.id) continue;
    events.push(event);
    if (event.type === 'session.error') assert.fail(event.data.message);
    if (event.type === 'session.status' && event.data.status === 'idle') return events;
  }
  assert.fail('stream ended before the turn completed');
}

test('installed package runs bundled Pi and application tools, then recovers sessions', { timeout: 90000 }, async t => {
  const f = await provider(t);
  let brain = await BrainServer.start(f.options);
  t.after(() => brain.stop());
  assert.match(brain.url, /^http:\/\/127\.0\.0\.1:\d+$/);
  const values = [];
  const echo = Tool({
    name: 'echo', description: 'Echo text', input_schema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'] },
    run: args => { values.push(args.text); return { content: [{ type: 'text', text: args.text }] }; },
  });
  const session = await brain.create({ harness: 'pi', model: Model('openai/gpt-4.1-mini'), tools: [echo] });
  const events = await turn(brain, session);
  assert.deepEqual(values, ['hello']);
  assert.ok(events.some(e => e.type === 'assistant.message' && e.data.text === 'hello from packaged Pi'));
  assert.ok(events.some(e => e.type === 'tool.result' && !e.data.is_error));
  await brain.stop();
  assert.equal((await brain.exited).code, 0);
  brain = await BrainServer.start(f.options);
  assert.equal((await brain.get(session.id)).id, session.id);
  await brain.rebind(session.id, [echo]);
  await turn(brain, session);
  assert.equal(f.requests.length, 4);
  await Promise.all([brain.stop(), brain.stop()]);
  assert.equal((await brain.exited).code, 0);
});

test('startup rejects invalid provider keys, invalid ports, and an occupied port', { timeout: 30000 }, async t => {
  await assert.rejects(BrainServer.start({ data_dir: './misplaced' }), /Unknown BrainServer option: data_dir/);
  await assert.rejects(BrainServer.start({ providers: { openai: { apiKey: 'key', base_url: 'https://proxy.example.com' } } }), /Unknown provider openai option: base_url/);
  await assert.rejects(BrainServer.start({ providers: { openai: { apiKey: '' } } }), /API key/);
  await assert.rejects(BrainServer.start({ port: -1 }), /port/);
  const f = await provider(t);
  const brain = await BrainServer.start(f.options);
  t.after(() => brain.stop());
  await assert.rejects(BrainServer.start({ ...f.options, port: Number(new URL(brain.url).port), dataDir: path.join(directory, 'occupied') }), /Brain exited/);
});

test('the child and active harness shut down after the parent script exits', { timeout: 60000 }, async t => {
  const f = await provider(t);
  const script = path.join(directory, 'parent.mjs');
  await writeFile(script, `import { BrainServer, Model } from 'brainserver';
const brain = await BrainServer.start(JSON.parse(process.argv[2]));
const session = await brain.create({harness:'pi', model:Model('openai/gpt-4.1-mini'), tools:[]});
await brain.send(session.id, {type:'user.message', text:'hello'});
for await (const e of brain.observe(session.id)) {
  if (e.type === 'session.error') throw Error(e.data.message);
  if (e.type === 'session.status' && e.data.status === 'idle') break;
}
console.log(brain.url);
process.exit(0);
`);
  const parent = spawn(process.execPath, [script, JSON.stringify(f.options)], { cwd: directory, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
  let output = '', errors = '';
  parent.stdout.on('data', chunk => { output += chunk; });
  parent.stderr.on('data', chunk => { errors += chunk; });
  const [code] = await once(parent, 'close');
  assert.equal(code, 0, errors);
  const url = output.trim();
  assert.match(url, /^http:\/\/127\.0\.0\.1:\d+$/);
  const deadline = Date.now() + 10000;
  while (true) {
    try { await fetch(url + '/health/live', { signal: AbortSignal.timeout(1000) }); }
    catch { break; }
    assert.ok(Date.now() < deadline, 'orphaned server still listening');
    await delay(25);
  }
  // A clean shutdown releases the session store for a new server.
  const brain = await BrainServer.start(f.options);
  t.after(() => brain.stop());
  assert.equal((await brain.list()).sessions.length, 1);
});
