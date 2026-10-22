import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createReadStream } from 'node:fs';
import { access, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import http from 'node:http';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { after, before, test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { responsesReply } from './journeys/model-responses.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const consumerRoot = path.resolve(process.env.RUNNER_TEMP ?? tmpdir());
let directory, BrainServer, Brain, Pi, Codex, OpenCode, PiWebAccess, PiSubagents, PiMcp, Model, ApiKey, Tool, AwsS3, Modal, E2B, Daytona;
before(async () => {
  directory = await mkdtemp(path.join(consumerRoot, 'brainserver consumer '));
  await writeFile(path.join(directory, 'package.json'), JSON.stringify({ private: true, type: 'module' }));
  const release = JSON.parse(await readFile(path.join(root, 'dist/server/release.json'), 'utf8'));
  const packages = release.npmPackages.filter(p => p.name === 'brainserver');
  for (const name of ['brainserver', `@aexhq/brainserver-${process.platform}-${process.arch}`, `@aexhq/brainserver-codex-${process.platform}-${process.arch}`]) {
    assert.ok(release.npmPackages.some(p => p.name === name), `package ${name} before testing`);
  }
  // Exercise registry installation. npm's file-tarball fetcher first extracts the
  // entire runtime into a temporary cache just to discover its manifest.
  const registry = http.createServer((req, res) => {
    const pathname = decodeURIComponent(new URL(req.url, 'http://registry').pathname.slice(1));
    const archive = release.npmPackages.find(p => pathname === `archives/${p.file}`);
    if (archive) { createReadStream(path.join(root, 'dist/server', archive.file)).on('error', error => res.destroy(error)).pipe(res); return; }
    const item = release.npmPackages.find(p => p.name === pathname);
    if (!item) { res.writeHead(404); res.end(); return; }
    const version = item.manifest.version;
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ name: item.name, 'dist-tags': { latest: version }, versions: {
      [version]: { ...item.manifest, dist: { tarball: `http://127.0.0.1:${registry.address().port}/archives/${item.file}`, integrity: item.integrity } },
    } }));
  });
  registry.listen(0, '127.0.0.1'); await once(registry, 'listening');
  try {
    const args = ['install', '--registry', `http://127.0.0.1:${registry.address().port}`, '--ignore-scripts', '--no-audit', '--no-fund', ...packages.map(p => `${p.name}@${p.manifest.version}`)];
    const command = process.platform === 'win32' ? process.execPath : 'npm';
    if (process.platform === 'win32') args.unshift(path.join(path.dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'));
    const installed = spawn(command, args, { cwd: directory, timeout: 900000, windowsHide: true });
    let output = '';
    for (const stream of [installed.stdout, installed.stderr]) stream.on('data', bytes => { output += bytes; });
    const [code, signal] = await once(installed, 'close');
    assert.equal(code, 0, output || `npm installation stopped: ${signal}`);
  } finally { registry.closeAllConnections(); await new Promise(resolve => registry.close(resolve)); }
  const require = createRequire(path.join(directory, 'package.json'));
  ({ BrainServer, Brain, Pi, Codex, OpenCode, PiWebAccess, PiSubagents, PiMcp, Model, ApiKey, Tool, AwsS3, Modal, E2B, Daytona } = await import(pathToFileURL(require.resolve('brainserver')).href));
}, { timeout: 930000 });
after(async () => {
  if (directory) {
    assert.equal(path.dirname(directory), consumerRoot);
    await rm(directory, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
});

test('installed TypeScript declarations expose the server and client without extra dependencies', async () => {
  await writeFile(path.join(directory, 'consumer.ts'), `import { Pi, BrainServer, Model, AwsS3, Modal, E2B, Daytona, type Session } from 'brainserver';
const brain: BrainServer = new BrainServer({providers:{openai:{apiKey:'key'}}, tools:[AwsS3(), Modal(), E2B(), Daytona()]});
await brain.start();
const session: Session = await brain.create({harness: Pi(), model:Model('openai/gpt-4.1-mini'), tools:[]});
void session;
await brain.stop();
`);
  const checked = spawnSync(process.execPath, [path.join(root, 'sdks/typescript/node_modules/typescript/bin/tsc'), '--noEmit', '--strict', '--module', 'NodeNext', '--target', 'ES2023', 'consumer.ts'], { cwd: directory, encoding: 'utf8' });
  assert.equal(checked.status, 0, checked.stdout + checked.stderr);
});

async function provider(t, api = 'openai-completions') {
  const requests = [];
  const fixture = { requests, toolCall: undefined };
  const errors = [];
  const server = http.createServer(async (req, res) => {
    try {
      assert.equal(req.url, api === 'openai-responses' ? '/v1/responses' : '/v1/chat/completions');
      assert.equal(req.headers.authorization, 'Bearer package-test-provider');
      let body = '';
      for await (const chunk of req) body += chunk;
      const input = JSON.parse(body);
      requests.push(input);
      const last = (input.messages ?? input.input).at(-1);
      const call = last.role === 'user' && input.tools?.length;
      if (api === 'openai-responses') {
        res.modelStreaming = input.stream !== false;
        responsesReply(res, call ? fixture.toolCall?.(input) ?? { name: input.tools[0].name, args: { text: 'hello' } } : 'hello from packaged harness', { input_tokens: 20, output_tokens: 5, total_tokens: 25 });
        return;
      }
      res.writeHead(200, { 'Content-Type': 'text/event-stream' });
      const chunk = (delta, finish_reason = null) => res.write(`data: ${JSON.stringify({
        id: 'package-test', object: 'chat.completion.chunk', created: 1, model: 'gpt-4.1-mini',
        choices: [{ index: 0, delta, finish_reason }],
      })}\n\n`);
      chunk({ role: 'assistant' });
      if (call) {
        const selected = fixture.toolCall?.(input) ?? { name: input.tools[0].function.name, args: { text: 'hello' } };
        chunk({ tool_calls: [{ index: 0, id: 'call-package', type: 'function', function: { name: selected.name, arguments: JSON.stringify(selected.args) } }] });
      }
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
  fixture.options = {
    dataDir: path.join(directory, t.name),
    providers: { openai: { apiKey: 'package-test-provider', baseUrl: `http://127.0.0.1:${server.address().port}/v1`, api } },
  };
  return fixture;
}

for (const harness of ['codex', 'opencode']) {
  test(`installed BrainServer runs ${harness} with callbacks and passive restart`, { timeout: 120000 }, async t => {
    const f = await provider(t, 'openai-responses');
    const selected = harness === 'codex' ? Codex({ name: 'configured-codex', maxShared: 2 }) : OpenCode({ name: 'configured-opencode', maxShared: 2 });
    let brain = new BrainServer({ ...f.options, harnesses: [selected] });
    t.after(() => brain.stop());
    await brain.start();
    const echo = { name: 'echo', description: 'Echo text', input_schema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'], additionalProperties: false }, run: args => args.text };
    const session = await brain.create({ harness: selected, model: Model('openai/gpt-4.1-mini'), tools: [echo] });
    const events = await turn(brain, session);
    assert.ok(events.some(event => event.type === 'tool.result' && !event.data.result.is_error));
    assert.equal(f.requests.length, 2);
    await brain.stop();
    brain = await new BrainServer({ ...f.options, harnesses: [selected] }).start();
    await brain.get(session.id);
    assert.equal(f.requests.length, 2, 'restart called the model');
    await brain.rebind(session.id, [echo]);
    await turn(brain, session);
    assert.equal(f.requests.length, 4);
  });
}

for (const extension of ['web-access', 'subagents', 'mcp']) {
  test(`installed Pi ${extension} tools are recorded and recover after restart`, { timeout: 120000 }, async t => {
    const f = await provider(t);
    const page = http.createServer(async (req, res) => {
      if (extension === 'mcp') {
        if (req.method !== 'POST') { res.writeHead(405); res.end(); return; }
        let body = ''; for await (const chunk of req) body += chunk;
        const input = JSON.parse(body);
        if (input.id === undefined) { res.writeHead(202); res.end(); return; }
        const result = input.method === 'initialize'
          ? { protocolVersion: input.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'brain-package-mcp', version: '1' } }
          : input.method === 'tools/list'
            ? { tools: [{ name: 'echo', description: 'Echo text', inputSchema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'] } }] }
            : { content: [{ type: 'text', text: 'verified MCP plugin ' + input.params.arguments.text }] };
        res.writeHead(200, { 'Content-Type': 'application/json' }); res.end(JSON.stringify({ jsonrpc: '2.0', id: input.id, result }));
      } else {
        res.writeHead(200, { 'Content-Type': 'text/html' }); res.end('<html><title>Brain fixture</title><body><article>Verified plugin fetch content from the local fixture.</article></body></html>');
      }
    });
    page.listen(0, '127.0.0.1'); await once(page, 'listening');
    const plugin = extension === 'subagents' ? PiSubagents({ config: { agents: { brain_worker: {
      description: 'Delegated callback task', systemPrompt: 'Complete the task.', tools: ['runtime_echo'],
    } } } })
      : extension === 'mcp' ? PiMcp({ config: { mcpServers: { demo: { url: `http://127.0.0.1:${page.address().port}/mcp`, tools: ['echo'] } } } })
      : PiWebAccess({ tools: ['fetch_content'], config: { ssrf: { allowRanges: ['127.0.0.0/8'] } } });
    const pi = Pi({ plugins: [plugin] });
    t.after(() => { page.closeAllConnections(); return new Promise(resolve => page.close(resolve)); });
    f.toolCall = input => extension === 'subagents'
      ? input.tools.some(tool => tool.function.name === 'subagent')
        ? { name: 'subagent', args: { agent: 'brain_worker', task: 'Say hello.', async: false } }
        : { name: 'runtime_echo', args: { text: 'child' } }
      : extension === 'mcp' ? { name: input.tools.find(tool => tool.function.name.includes('echo')).function.name, args: { text: 'test' } }
        : { name: 'fetch_content', args: { url: `http://127.0.0.1:${page.address().port}/page`, mode: 'raw' } };
    const childCalls = [];
    const echo = { name: 'echo', description: 'Echo child text',
      input_schema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'] },
      run: args => { childCalls.push(args.text); return args.text; },
    };
    const tools = extension === 'subagents' ? [echo] : [];
    let brain = await new BrainServer({ ...f.options, harnesses: [pi] }).start();
    t.after(() => brain.stop());
    const session = await brain.create({ harness: pi, model: Model('openai/gpt-4.1-mini'), tools });
    const events = await turn(brain, session);
    const executed = events.filter(event => event.type === 'tool.result').at(-1);
    assert.ok(executed, 'plugin result was not recorded');
    assert.ok(!executed.data.result.is_error, JSON.stringify(executed.data));
    assert.equal(f.requests.length, extension === 'subagents' ? 4 : 2);
    assert.match(JSON.stringify(executed.data), extension === 'subagents' ? /hello from packaged Pi/ : extension === 'mcp' ? /verified MCP plugin/ : /Verified plugin fetch/);
    const requests = f.requests.length;
    await brain.stop();
    brain = await new BrainServer({ ...f.options, harnesses: [pi] }).start();
    await brain.get(session.id);
    assert.equal(f.requests.length, requests);
    if (tools.length) await brain.rebind(session.id, tools);
    await turn(brain, session);
    assert.ok(f.requests.length > requests);
    assert.equal((await brain.history(session.id)).events.filter(event => event.type === 'tool.call').length, extension === 'subagents' ? 4 : 2);
    if (extension === 'subagents') {
      assert.deepEqual(childCalls, ['child', 'child']);
      const before = f.requests.length;
      const marker = path.join(directory, 'unbound-command.txt');
      f.toolCall = () => ({ name: 'subagent', args: { agent: 'brain_worker', task: 'Say hello.', async: false,
        options: { gate: `"${process.execPath}" -e "require('node:fs').writeFileSync('${marker.replaceAll('\\', '/')}', 'unbound')"` },
      } });
      const rejected = await turn(brain, session);
      assert.ok(rejected.some(event => event.type === 'tool.result' && event.data.result.is_error));
      assert.equal(f.requests.length, before + 2, 'rejected host controls launched child inference');
      await assert.rejects(access(marker), { code: 'ENOENT' });
    }
  });
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
  let brain = await new BrainServer(f.options).start();
  t.after(() => brain.stop());
  assert.match(brain.url, /^http:\/\/127\.0\.0\.1:\d+$/);
  const values = [];
  const echo = Tool({
    name: 'echo', description: 'Echo text', input_schema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'] },
    run: args => { values.push(args.text); return { content: [{ type: 'text', text: args.text }] }; },
  });
  const session = await brain.create({ harness: Pi(), model: Model('openai/gpt-4.1-mini'), tools: [echo] });
  const events = await turn(brain, session);
  assert.deepEqual(values, ['hello']);
  assert.ok(events.some(e => e.type === 'assistant.message' && e.data.text === 'hello from packaged Pi'));
  assert.ok(events.some(e => e.type === 'tool.result' && !e.data.result.is_error));
  await brain.stop();
  assert.equal((await brain.exited).code, 0);
  brain = await new BrainServer(f.options).start();
  assert.equal((await brain.get(session.id)).id, session.id);
  await brain.rebind(session.id, [echo]);
  await turn(brain, session);
  assert.equal(f.requests.length, 4);
  await Promise.all([brain.stop(), brain.stop()]);
  assert.equal((await brain.exited).code, 0);
});

test('startup rejects invalid provider keys, invalid ports, and an occupied port', { timeout: 30000 }, async t => {
  assert.throws(() => new BrainServer({ data_dir: './misplaced' }), /Unknown BrainServer option: data_dir/);
  assert.throws(() => new BrainServer({ providers: { openai: { apiKey: 'key', base_url: 'https://proxy.example.com' } } }), /Unknown provider openai option: base_url/);
  assert.throws(() => new BrainServer({ providers: { openai: { apiKey: '' } } }), /API key/);
  assert.throws(() => new BrainServer({ port: -1 }), /port/);
  const f = await provider(t);
  const brain = await new BrainServer(f.options).start();
  t.after(() => brain.stop());
  await assert.rejects(new BrainServer({ ...f.options, port: Number(new URL(brain.url).port), dataDir: path.join(directory, 'occupied') }).start(), /Brain child exited/);
});

test('construction installs a flat list without starting processes or requiring tool credentials', async () => {
  const dataDir = path.join(directory, 'not-started');
  const brain = new BrainServer({ dataDir, providers: { openai: {} }, tools: [Modal(), AwsS3(), E2B(), Daytona()] });
  await assert.rejects(access(dataDir), { code: 'ENOENT' });
  assert.throws(() => brain.exited, /has not started/);
  await brain.stop();
  await assert.rejects(brain.start(), /stopped/);
});

test('remote SDK sessions use optional defaults and encrypted credential overrides across restart', { timeout: 90000 }, async t => {
  const f = await provider(t);
  const apiKey = 'remote-sdk-client-authentication-token';
  const options = { ...f.options, apiKey, tools: [
    AwsS3({ bucket: 'installed-bucket', region: 'eu-west-2', accessKeyId: 'server-s3-access', secretAccessKey: 'server-s3-private-secret' }),
    Modal({ tokenId: 'server-modal-id', tokenSecret: 'server-modal-private-secret' }),
  ] };
  let server = new BrainServer(options);
  const starting = server.start();
  assert.equal(server.start(), starting);
  await starting;
  t.after(() => server.stop());
  let client = new Brain(server.url, apiKey);
  const defaults = await client.create({ harness: Pi(), model: Model('openai/gpt-4.1-mini'), tools: [AwsS3(), Modal()] });
  const privateSession = await client.create({ harness: Pi(), model: Model('openai/gpt-4.1-mini'), tools: [
    AwsS3({ accessKeyId: 'session-s3-access', secretAccessKey: 'session-s3-private-secret', sessionToken: 'session-temporary-token', operations: ['read'] }),
    Modal({ tokenId: 'session-modal-id', tokenSecret: 'session-modal-private-secret', operations: ['inspect'] }),
  ] });
  assert.deepEqual(privateSession.descriptors.map(tool => tool.name), ['aws_s3_read', 'modal_inspect']);
  assert.doesNotMatch(JSON.stringify(privateSession), /session-s3-access|private-secret|temporary-token|session-modal-id/);
  await assert.rejects(client.create({ harness: Pi(), model: Model('openai/gpt-4.1-mini'), tools: [AwsS3({ accessKeyId: 'incomplete-override' })] }), error => error.status === 403);
  assert.equal(f.requests.length, 0);
  await server.stop();
  const dataFiles = async dir => {
    const result = [];
    for (const entry of await readdir(dir, { withFileTypes: true })) {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) result.push(...await dataFiles(file));
      else result.push(await readFile(file));
    }
    return result;
  };
  for (const data of await dataFiles(options.dataDir)) {
    for (const secret of ['session-s3-private-secret', 'session-modal-private-secret', 'session-temporary-token']) assert.equal(data.includes(Buffer.from(secret)), false, 'plaintext credential on disk');
  }
  server = await new BrainServer(options).start();
  client = new Brain(server.url, apiKey);
  assert.equal((await client.get(privateSession.id)).id, privateSession.id);
  assert.equal((await client.get(defaults.id)).id, defaults.id);
  // The reused sandbox service rejects a forged handle without a cloud action.
  f.toolCall = () => ({ name: 'modal_inspect', args: { sandbox: 'forged-handle' } });
  const events = await turn(client, privateSession);
  assert.ok(events.some(event => event.type === 'tool.result' && event.data.result.is_error));
  assert.doesNotMatch(JSON.stringify(f.requests), /session-s3-access|private-secret|temporary-token|session-modal-id/);
});

test('integrations without defaults accept session credentials and reject missing or malformed overrides', { timeout: 60000 }, async t => {
  const f = await provider(t);
  const server = await new BrainServer({ ...f.options,
    providers: { openai: { ...f.options.providers.openai, apiKey: undefined } },
    tools: [AwsS3(), Modal(), E2B(), Daytona()],
  }).start();
  t.after(() => server.stop());
  for (const selection of [AwsS3(), Modal(), E2B(), Daytona()]) {
    await assert.rejects(server.create({ harness: Pi(), model: Model('openai/gpt-4.1-mini', ApiKey('package-test-provider')), tools: [selection] }), error => error.status === 403);
  }
  const session = await server.create({ harness: Pi(), model: Model('openai/gpt-4.1-mini', ApiKey('package-test-provider')), tools: [
    AwsS3({ bucket: 'session-bucket', region: 'us-east-1', accessKeyId: 'session-access', secretAccessKey: 'private-session-key' }),
    Modal({ tokenId: 'session-id', tokenSecret: 'private-modal-key' }),
    E2B({ apiKey: 'private-e2b-key' }), Daytona({ apiKey: 'private-daytona-key' }),
  ] });
  assert.equal(session.descriptors.length, 14);
  assert.equal(f.requests.length, 0, 'binding integrations caused inference');
});

test('the child and active harness shut down after the parent script exits', { timeout: 60000 }, async t => {
  const f = await provider(t);
  f.options.tools = [Modal()];
  const script = path.join(directory, 'parent.mjs');
  await writeFile(script, `import { Pi, BrainServer, Model } from 'brainserver';
const brain = await new BrainServer(JSON.parse(process.argv[2])).start();
const session = await brain.create({harness: Pi(), model:Model('openai/gpt-4.1-mini'), tools:[]});
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
  const brain = await new BrainServer(f.options).start();
  t.after(() => brain.stop());
  assert.equal((await brain.list()).sessions.length, 1);
});
