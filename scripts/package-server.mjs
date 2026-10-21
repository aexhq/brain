import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmod, copyFile, cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
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
run('npm', ['--prefix', 'sdks/typescript', 'run', 'build']);
const pkg = JSON.parse(await readFile(path.join(root, 'packages/brainserver/package.json'), 'utf8'));
const pi = JSON.parse(await readFile(path.join(root, 'runners/pi/package.json'), 'utf8'));
const archives = [];
async function pack(directory, name) {
  const [result] = JSON.parse(run('npm', ['pack', '--ignore-scripts', '--json', '--pack-destination', output], directory, {}, true));
  archives.push({ name, file: result.filename });
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
  await writeFile(path.join(directory, 'package.json'), JSON.stringify({
    name, version: pkg.version, description: `Brain executable for ${target}`, license: 'MIT',
    os: [os], cpu: [arch], files: [path.basename(binary), 'LICENSE'], publishConfig: { access: 'public' },
    repository: pkg.repository,
  }, null, 2) + '\n');
  await copyFile(path.join(root, 'LICENSE'), path.join(directory, 'LICENSE'));
  await pack(directory, name);
}
const main = path.join(stage, 'main');
await mkdir(main);
// Bundle the lockfile's production dependency tree, rather than resolving Pi's
// transitive version ranges on each user's machine. No install scripts run.
for (const name of ['package.json', 'package-lock.json']) await copyFile(path.join(root, 'runners/pi', name), path.join(main, name));
run('npm', ['ci', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund'], main);
await cp(path.join(root, 'packages/brainserver'), main, { recursive: true });
await cp(path.join(root, 'sdks/typescript/dist'), path.join(main, 'sdk'), { recursive: true });
await cp(path.join(root, 'runners/pi/dist'), path.join(main, 'runners/pi/dist'), { recursive: true });
await cp(path.join(root, 'runners/common'), path.join(main, 'runners/common'), { recursive: true });
await copyFile(path.join(root, 'LICENSE'), path.join(main, 'LICENSE'));
await rm(path.join(main, 'package-lock.json'));
await writeFile(path.join(main, 'package.json'), JSON.stringify({
  ...pkg, dependencies: pi.dependencies, bundleDependencies: Object.keys(pi.dependencies),
  optionalDependencies: Object.fromEntries(targets.map(target => [`@aexhq/brainserver-${target}`, pkg.version])),
}, null, 2) + '\n');
const packed = await pack(main, pkg.name);
for (const name of ['index.js', 'index.d.ts', 'sdk/index.js', 'sdk/index.d.ts', 'runners/pi/dist/main.js', 'runners/common/media.mjs']) {
  assert.ok(packed.files.some(file => file.path === name), `server package missing ${name}`);
}
assert.ok(packed.bundled.includes('@earendil-works/pi-coding-agent'), 'Pi dependencies were not bundled');
const commit = run('git', ['rev-parse', 'HEAD'], root, {}, true).trim();
await writeFile(path.join(output, 'release.json'), JSON.stringify({ version: pkg.version, commit, npmPackages: archives }, null, 2) + '\n');
console.log(`Packaged brainserver ${pkg.version} (${selected.join(', ')}) in ${output}`);
