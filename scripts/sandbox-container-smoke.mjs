import { spawnSync } from 'node:child_process';
import assert from 'node:assert/strict';

const name = `brain-sandbox-smoke-${process.pid}`;
function docker(...args) {
  const output = spawnSync('docker', args, { encoding: 'utf8' });
  if (output.status !== 0) throw new Error(output.stderr);
  return output.stdout.trim();
}
try {
  docker('run', '-d', '--name', name, '-p', '127.0.0.1::8091', '-e', 'TOOL_API_KEY=fixture-token-with-32-characters!!', '-e', 'SANDBOX_SIGNING_KEY=fixture-signing-key-with-32-characters!!', '-e', 'E2B_API_KEY=fixture-unused', 'brain-sandbox-tools:local-dev');
  const port = JSON.parse(docker('inspect', name))[0].NetworkSettings.Ports['8091/tcp'][0].HostPort;
  const url = `http://127.0.0.1:${port}`;
  let ready = false;
  for (let i = 0; i < 100; i++) {
    try { if ((await fetch(url + '/health')).status === 200) { ready = true; break; } } catch {}
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert.ok(ready, 'sandbox tool container never became healthy');
  assert.equal((await fetch(url + '/create', { method: 'POST' })).status, 401);
  assert.equal(docker('exec', name, 'id', '-u'), '1000');
  console.log('sandbox tools: healthy, authenticated, unprivileged');
} finally { docker('rm', '-f', name); }
