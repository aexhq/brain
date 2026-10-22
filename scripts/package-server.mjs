import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmod, copyFile, cp, mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const stage = path.join(root, 'bin/brainserver-build');
const output = path.join(root, 'dist/server');
const targets = ['linux-x64', 'linux-arm64', 'darwin-x64', 'darwin-arm64', 'win32-x64', 'win32-arm64'];
const host = `${process.platform}-${process.arch}`;
assert.ok(targets.includes(host), `unsupported build host: ${host}`);
assert.ok(process.argv.slice(2).every(arg => arg === '--all-platforms'), 'unknown packaging argument');
const selected = process.argv.includes('--all-platforms') ? targets : [host];
function run(command, args, cwd = root, extraEnv = {}, capture = false) {
  if (command === 'npm' && process.platform === 'win32') {
    args = [path.join(path.dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'), ...args];
    command = process.execPath;
  }
  const result = spawnSync(command, args, { cwd, env: { ...process.env, ...extraEnv }, encoding: 'utf8', stdio: capture ? 'pipe' : 'inherit', maxBuffer: 16 << 20 });
  if (result.error) throw result.error;
  assert.equal(result.status, 0, `${command} ${args.join(' ')}\n${result.stderr ?? ''}`);
  return result.stdout;
}
// This script owns only these two generated directories.
for (const directory of [stage, output]) {
  assert.ok(path.relative(root, directory) && !path.relative(root, directory).startsWith('..'));
  await rm(directory, { recursive: true, force: true });
  await mkdir(directory, { recursive: true });
}
run('npm', ['--prefix', 'runners/pi', 'run', 'build']);
run('npm', ['--prefix', 'runners/codex', 'run', 'build']);
run('npm', ['--prefix', 'runners/opencode', 'run', 'build']);
run('npm', ['--prefix', 'sdks/typescript', 'run', 'build']);
const pkg = JSON.parse(await readFile(path.join(root, 'packages/brainserver/package.json'), 'utf8'));
const pi = JSON.parse(await readFile(path.join(root, 'runners/pi/package.json'), 'utf8'));
const sandbox = JSON.parse(await readFile(path.join(root, 'services/sandbox/package.json'), 'utf8'));
const codex = JSON.parse(await readFile(path.join(root, 'runners/codex/package.json'), 'utf8'));
const opencode = JSON.parse(await readFile(path.join(root, 'runners/opencode/package.json'), 'utf8'));
for (const [name, version] of Object.entries({ ...pi.dependencies, ...sandbox.dependencies, ...codex.dependencies, ...opencode.dependencies })) {
  assert.equal(pkg.dependencies[name], version, `brainserver must pin ${name} to the integration's version`);
}
const archives = [];
const productionLock = JSON.parse(await readFile(path.join(root, 'packages/brainserver/package-lock.json'), 'utf8'));
async function pack(directory, name) {
  const [result] = JSON.parse(run('npm', ['pack', '--ignore-scripts', '--json', '--pack-destination', output], directory, {}, true));
  assert.ok(result.size < 180 * 1024 * 1024, `${name} exceeds the native archive upload budget; split its runtime before publishing`);
  archives.push({ name, file: result.filename, integrity: result.integrity, manifest: JSON.parse(await readFile(path.join(directory, 'package.json'), 'utf8')) });
  return result;
}
for (const target of selected) {
  const [os, arch] = target.split('-');
  const directory = path.join(stage, target);
  await mkdir(directory);
  const binary = path.join(directory, os === 'win32' ? 'brain.exe' : 'brain');
  run('go', ['build', '-trimpath', '-o', binary, './cmd/brain'], root, {
    CGO_ENABLED: '0', GOOS: os === 'win32' ? 'windows' : os, GOARCH: arch === 'x64' ? 'amd64' : arch,
  });
  await chmod(binary, 0o755);
  const name = `@aexhq/brainserver-${target}`;
  const manifest = {
    name, version: pkg.version, description: `Brain and native harness runtimes for ${target}`, license: 'MIT', type: 'module',
    os: [os], cpu: [arch], files: [path.basename(binary), 'runtime', 'LICENSE'], publishConfig: { access: 'public' },
    repository: pkg.repository,
  };
  // npm checks the root against the build host before applying dependency targets.
  // Add the archive's platform restriction after installing its target tree.
  await writeFile(path.join(directory, 'package.json'), JSON.stringify({ ...manifest, os: undefined, cpu: undefined, dependencies: pkg.dependencies, overrides: pkg.overrides }, null, 2) + '\n');
  const lock = structuredClone(productionLock);
  lock.name = name;
  lock.packages[''] = { ...lock.packages[''], name };
  await writeFile(path.join(directory, 'package-lock.json'), JSON.stringify(lock, null, 2) + '\n');
  run('npm', ['ci', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund', `--os=${os}`, `--cpu=${arch}`], directory);
  await writeFile(path.join(directory, 'package.json'), JSON.stringify(manifest, null, 2) + '\n');
  // Keep the locked runtime private: npm must not resolve its bundled peer graph.
  const runtime = path.join(directory, 'runtime');
  await mkdir(runtime);
  await rename(path.join(directory, 'node_modules'), path.join(runtime, 'node_modules'));
  await cp(path.join(root, 'runners/pi/dist'), path.join(runtime, 'runners/pi/dist'), { recursive: true });
  await cp(path.join(root, 'runners/opencode/src'), path.join(runtime, 'runners/opencode/src'), { recursive: true });
  await cp(path.join(root, 'runners/common'), path.join(runtime, 'runners/common'), { recursive: true });
  await mkdir(path.join(runtime, 'services'), { recursive: true });
  await copyFile(path.join(root, 'services/http.mjs'), path.join(runtime, 'services/http.mjs'));
  await cp(path.join(root, 'services/sandbox/src'), path.join(runtime, 'services/sandbox/src'), { recursive: true });
  await copyFile(path.join(root, 'packages/brainserver/sandbox.js'), path.join(runtime, 'sandbox.js'));
  await copyFile(path.join(root, 'LICENSE'), path.join(directory, 'LICENSE'));
  // Codex's native executable makes the combined archive exceed npm's upload
  // limit. Keep its pinned tree opaque in an automatically installed companion.
  const codexDirectory = path.join(stage, `codex-${target}`);
  const codexRuntime = path.join(codexDirectory, 'runtime');
  await mkdir(path.join(codexRuntime, 'node_modules'), { recursive: true });
  await rename(path.join(runtime, 'node_modules/@openai'), path.join(codexRuntime, 'node_modules/@openai'));
  await cp(path.join(root, 'runners/codex/src'), path.join(codexRuntime, 'runners/codex/src'), { recursive: true });
  await cp(path.join(root, 'runners/common'), path.join(codexRuntime, 'runners/common'), { recursive: true });
  await copyFile(path.join(root, 'LICENSE'), path.join(codexDirectory, 'LICENSE'));
  const codexName = `@aexhq/brainserver-codex-${target}`;
  await writeFile(path.join(codexDirectory, 'package.json'), JSON.stringify({ ...manifest, name: codexName,
    description: `Brain's pinned Codex harness runtime for ${target}`, files: ['runtime', 'LICENSE'] }, null, 2) + '\n');
  const codexPacked = await pack(codexDirectory, codexName);
  for (const file of ['runners/codex/src/main.mjs', 'runners/common/bridge.mjs', 'node_modules/@openai/codex/package.json']) assert.ok(codexPacked.files.some(entry => entry.path === `runtime/${file}`), `Codex runtime missing ${file}`);
  const packed = await pack(directory, name);
  for (const file of ['runners/pi/dist/main.js', 'runners/opencode/src/main.mjs', 'runners/common/bridge.mjs']) assert.ok(packed.files.some(entry => entry.path === `runtime/${file}`), `runtime missing ${file}`);
  for (const dependency of Object.keys(pkg.dependencies).filter(name => name !== '@openai/codex')) assert.ok(packed.files.some(entry => entry.path === `runtime/node_modules/${dependency}/package.json`), `runtime did not include ${dependency}`);
}
const main = path.join(stage, 'main');
await mkdir(main);
// The portable entry point has no runtime dependencies outside its selected
// platform packages. Their complete locked native trees stay opaque to npm.
for (const file of ['index.js', 'index.d.ts', 'process.js', 'README.md']) await copyFile(path.join(root, 'packages/brainserver', file), path.join(main, file));
await cp(path.join(root, 'sdks/typescript/dist'), path.join(main, 'sdk'), { recursive: true });
await copyFile(path.join(root, 'LICENSE'), path.join(main, 'LICENSE'));
const manifest = {
  ...pkg, dependencies: undefined, overrides: undefined,
  optionalDependencies: Object.fromEntries(targets.flatMap(target => [
    [`@aexhq/brainserver-${target}`, pkg.version], [`@aexhq/brainserver-codex-${target}`, pkg.version],
  ])),
};
await writeFile(path.join(main, 'package.json'), JSON.stringify(manifest, null, 2) + '\n');
const packed = await pack(main, pkg.name);
for (const name of ['index.js', 'index.d.ts', 'sdk/index.js', 'sdk/index.d.ts']) {
  assert.ok(packed.files.some(file => file.path === name), `server package missing ${name}`);
}
const commit = run('git', ['rev-parse', 'HEAD'], root, {}, true).trim();
await writeFile(path.join(output, 'release.json'), JSON.stringify({ version: pkg.version, commit, npmPackages: archives }, null, 2) + '\n');
console.log(`Packaged brainserver ${pkg.version} (${selected.join(', ')}) in ${output}`);
