import { spawnSync } from 'node:child_process';

for (const directory of ['runners/pi', 'runners/codex', 'runners/opencode', 'sdks/typescript', 'services/sandbox', 'services/browser', 'website']) {
  const result = spawnSync('npm', ['--prefix', directory, 'ci', '--ignore-scripts'], { stdio: 'inherit', shell: process.platform === 'win32' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
const browser = spawnSync('npm', ['--prefix', 'services/browser', 'run', 'install-browser'], { stdio: 'inherit', shell: process.platform === 'win32' });
if (browser.error) throw browser.error;
if (browser.status !== 0) process.exit(browser.status ?? 1);
