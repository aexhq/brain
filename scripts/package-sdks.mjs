import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const artifacts = path.join(root, 'dist/sdk');
const consumer = path.join(root, 'bin/sdk-consumer');
function run(command, args, cwd = root, capture = false) {
  if (command === 'npm' && process.platform === 'win32') {
    args = [path.join(path.dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'), ...args];
    command = process.execPath;
  }
  const result = spawnSync(command, args, { cwd, env: process.env, encoding: 'utf8', stdio: capture ? 'pipe' : 'inherit' });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${command} ${args.join(' ')}\n${result.stderr ?? ''}`);
  return result.stdout;
}

// Only this script's generated distribution directory is replaced.
await rm(artifacts, { recursive: true, force: true });
await mkdir(artifacts, { recursive: true });
await mkdir(consumer, { recursive: true });
const pkg = JSON.parse(await readFile(path.join(root, 'sdks/typescript/package.json'), 'utf8'));
assert.equal(await readFile(path.join(root, 'sdks/typescript/LICENSE'), 'utf8'), await readFile(path.join(root, 'LICENSE'), 'utf8'));
const [packed] = JSON.parse(run('npm', ['pack', '--ignore-scripts', '--json', '--pack-destination', artifacts], path.join(root, 'sdks/typescript'), true));
for (const file of ['dist/index.js', 'dist/index.d.ts', 'dist/generated.d.ts', 'dist/cli.mjs', 'dist/cli.d.mts', 'dist/telemetry.js', 'README.md', 'LICENSE']) {
  assert.ok(packed.files.some(item => item.path === file), `npm archive missing ${file}`);
}
assert.ok(!packed.files.some(item => /^(src|test|node_modules)\//.test(item.path)), 'unexpected npm package files');
// Consumer directories contain only installed packages, never a source checkout.
await writeFile(path.join(consumer, 'package.json'), JSON.stringify({ private: true, type: 'module' }));
run('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund', '--save-exact', path.join(artifacts, packed.filename)], consumer);
const tests = await readFile(path.join(root, 'sdks/typescript/test/client.test.mjs'), 'utf8');
await writeFile(path.join(consumer, 'client.test.mjs'), tests.replace('"../dist/index.js"', '"@aexhq/brain"'));
run(process.execPath, ['--test', 'client.test.mjs'], consumer);
run(process.execPath, ['--input-type=module', '-e', "import { main } from '@aexhq/brain/cli'; await main(['--help']);"], consumer);
  await writeFile(path.join(consumer, 'consumer.ts'), `import { Pi, Brain, BrainError, Model, ApiKey, Tool, ManagedTool, type Session, type Event } from '@aexhq/brain';
import {main, configureTelemetry} from '@aexhq/brain/cli';
const brain = new Brain('http://brain.test', 'token');
const cli: Promise<void> = main(['list'], {client: brain, output: () => {}});
configureTelemetry({});
const session: Promise<Session> = brain.get('id');
const events: AsyncGenerator<Event> = brain.observe('id');
const error: Error = new BrainError(409, 'conflict', 'busy');
  const declared: Promise<Session> = brain.create({ harness: Pi(), model: Model('openai/model', ApiKey('key')), tools: [ManagedTool('lookup'), Tool({name: 'local', description: 'Read', input_schema: {type: 'object'}, run: (_args, context) => ({tenant: context.scope.tenant_id})})] });
  const descriptor: Tool = {name: 'local', description: 'Read', input_schema: {type: 'object'}};
  void [session, events, error, declared, descriptor];\n`);
run(process.execPath, [path.join(root, 'sdks/typescript/node_modules/typescript/bin/tsc'), '--noEmit', '--strict', '--module', 'NodeNext', '--target', 'ES2023', 'consumer.ts'], consumer);
const commit = run('git', ['rev-parse', 'HEAD'], root, true).trim();
await writeFile(path.join(artifacts, 'release.json'), JSON.stringify({ version: pkg.version, commit, npm: packed.filename }, null, 2) + '\n');
console.log(`Verified installed SDK ${pkg.version} distributions at ${artifacts}`);
