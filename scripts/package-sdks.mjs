import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFile, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const artifacts = path.join(root, 'dist/sdk');
const consumer = path.join(root, 'bin/sdk-consumer');
const pythonIn = name => path.join(root, 'bin', name, process.platform === 'win32' ? 'Scripts/python.exe' : 'bin/python');
const buildPython = pythonIn('sdk-build');
const python = pythonIn('sdk-python');
const sourcePython = pythonIn('sdk-source');
// Fixed archive timestamps make unchanged sources reproducible across checkouts.
// Source identity is recorded separately in release.json.
const env = { ...process.env, PYTHONPATH: '', SOURCE_DATE_EPOCH: '315532800' };
// Resolve the Windows .bat shim once; subsequent paths never enter a shell.
const interpreter = spawnSync('python -c "import sys;print(sys.executable)"', { shell: true, encoding: 'utf8', env });
if (interpreter.error) throw interpreter.error;
assert.equal(interpreter.status, 0, interpreter.stderr);
const systemPython = interpreter.stdout.trim();
function run(command, args, cwd = root, capture = false) {
  if (command === 'python') command = systemPython;
  if (command === 'npm' && process.platform === 'win32') {
    args = [path.join(path.dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'), ...args];
    command = process.execPath;
  }
  const result = spawnSync(command, args, { cwd, env, encoding: 'utf8', stdio: capture ? 'pipe' : 'inherit' });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${command} ${args.join(' ')}\n${result.stderr ?? ''}`);
  return result.stdout;
}

await mkdir(artifacts, { recursive: true });
await mkdir(path.join(consumer, 'python'), { recursive: true });
const pkg = JSON.parse(await readFile(path.join(root, 'sdks/typescript/package.json'), 'utf8'));
const metadata = await readFile(path.join(root, 'sdks/python/pyproject.toml'), 'utf8');
assert.equal(metadata.match(/^version = "([^"]+)"/m)?.[1], pkg.version, 'SDK releases must share a version');
for (const sdk of ['typescript', 'python']) {
  assert.equal(await readFile(path.join(root, `sdks/${sdk}/LICENSE`), 'utf8'), await readFile(path.join(root, 'LICENSE'), 'utf8'));
}
const [packed] = JSON.parse(run('npm', ['pack', '--ignore-scripts', '--json', '--pack-destination', artifacts], path.join(root, 'sdks/typescript'), true));
for (const file of ['dist/index.js', 'dist/index.d.ts', 'dist/generated.d.ts', 'README.md', 'LICENSE']) {
  assert.ok(packed.files.some(item => item.path === file), `npm archive missing ${file}`);
}
assert.ok(!packed.files.some(item => /^(src|test|node_modules)\//.test(item.path)), 'unexpected npm package files');
run('python', ['-m', 'venv', path.dirname(path.dirname(buildPython))]);
run(buildPython, ['-m', 'pip', 'install', '--disable-pip-version-check', '-r', 'scripts/python-build-requirements.txt']);
run(buildPython, ['-m', 'build', '--no-isolation', '--outdir', artifacts, 'sdks/python']);
const wheel = `aex_brain-${pkg.version}-py3-none-any.whl`;
const source = `aex_brain-${pkg.version}.tar.gz`;
const files = await readdir(artifacts);
assert.ok(files.includes(wheel) && files.includes(source), 'missing Python distribution');

// Consumer directories contain only installed packages, never a source checkout.
await writeFile(path.join(consumer, 'package.json'), JSON.stringify({ private: true, type: 'module' }));
run('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund', '--save-exact', path.join(artifacts, packed.filename)], consumer);
const tests = await readFile(path.join(root, 'sdks/typescript/test/client.test.mjs'), 'utf8');
await writeFile(path.join(consumer, 'client.test.mjs'), tests.replace('"../dist/index.js"', '"@aexhq/brain"'));
run(process.execPath, ['--test', 'client.test.mjs'], consumer);
  await writeFile(path.join(consumer, 'consumer.ts'), `import { Pi, Brain, BrainError, Model, ApiKey, Tool, ManagedTool, type Session, type Event } from '@aexhq/brain';
const brain = new Brain('http://brain.test', 'token');
const session: Promise<Session> = brain.get('id');
const events: AsyncGenerator<Event> = brain.observe('id');
const error: Error = new BrainError(409, 'conflict', 'busy');
  const declared: Promise<Session> = brain.create({ harness: Pi(), model: Model('openai/model', ApiKey('key')), tools: [ManagedTool('lookup'), Tool({name: 'local', description: 'Read', input_schema: {type: 'object'}, run: (_args, context) => ({tenant: context.scope.tenant_id})})] });
  const descriptor: Tool = {name: 'local', description: 'Read', input_schema: {type: 'object'}};
  void [session, events, error, declared, descriptor];\n`);
run(process.execPath, [path.join(root, 'sdks/typescript/node_modules/typescript/bin/tsc'), '--noEmit', '--strict', '--module', 'NodeNext', '--target', 'ES2023', 'consumer.ts'], consumer);
await copyFile(path.join(root, 'sdks/python/test_client.py'), path.join(consumer, 'python/test_client.py'));
for (const [executable, archive] of [[python, wheel], [sourcePython, source]]) {
  run('python', ['-m', 'venv', path.dirname(path.dirname(executable))]);
  run(executable, ['-m', 'pip', 'install', '--disable-pip-version-check', '--no-deps', '--force-reinstall', path.join(artifacts, archive)]);
  run(executable, ['-I', '-c', `import brain, importlib.metadata as m, importlib.resources as r; assert m.version('aex-brain') == '${pkg.version}'; assert r.files('brain').joinpath('py.typed').is_file(); assert not m.requires('aex-brain'); print(brain.__file__)`], consumer);
  run(executable, ['-I', '-m', 'unittest', 'discover', '-s', 'python', '-p', 'test_*.py'], consumer);
}
const commit = run('git', ['rev-parse', 'HEAD'], root, true).trim();
await writeFile(path.join(artifacts, 'release.json'), JSON.stringify({ version: pkg.version, commit, npm: packed.filename, python: [wheel, source] }, null, 2) + '\n');
console.log(`Verified installed SDK ${pkg.version} distributions at ${artifacts}`);
