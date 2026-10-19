import { spawnSync } from 'node:child_process';

for (const directory of ['runners/pi', 'runners/codex', 'sdks/typescript', 'services/sandbox', 'website']) {
  const result = spawnSync('npm', ['--prefix', directory, 'ci', '--ignore-scripts'], { stdio: 'inherit', shell: process.platform === 'win32' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
