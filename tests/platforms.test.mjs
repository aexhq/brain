import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, rm } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import path from 'node:path';
import worker from '../examples/platforms/cloudflare/worker.mjs';

test('platform host configurations validate against the actual Brain executable', async t => {
  const directory = await mkdtemp(path.join(tmpdir(), 'brain-configs-'));
  t.after(async () => { if (path.dirname(directory) !== tmpdir()) throw Error('unexpected temporary directory'); await rm(directory, { recursive: true, force: true }); });
  const binary = path.join(directory, process.platform === 'win32' ? 'brain.exe' : 'brain');
  const build = spawnSync('go', ['build', '-o', binary, './cmd/brain'], { encoding: 'utf8' });
  assert.equal(build.status, 0, build.stderr);
  for (const platform of ['docker','fly','ecs']) {
    const config = `examples/platforms/${platform}/brain.json`;
    const result = spawnSync(binary, ['-check', '-config', config], { encoding: 'utf8', env: { ...process.env, BRAIN_API_KEY:'fixture-token-with-at-least-32-bytes', OPENAI_API_KEY:'fixture', RESOURCE_TOOL_TOKEN:'fixture', ARCHIVE_ACCESS_KEY:'fixture', ARCHIVE_SECRET_KEY:'fixture' } });
    assert.equal(result.status, 0, `${platform}: ${result.stderr}`);
  }
  for (const file of ['docker/compose.json','ecs/task-definition.json','cloudflare/wrangler.json']) JSON.parse(await readFile(`examples/platforms/${file}`, 'utf8'));
});

test('Cloudflare R2 tool Worker authenticates, isolates tenants, bounds bodies and respects read-only policy', async () => {
  const objects = new Map();
  const env = {
    TOOL_API_KEY: 'fixture-token-with-at-least-32-bytes',
    ALLOW_WRITE: 'true',
    BUCKET: {
      async put(key, value) { objects.set(key, value); },
      async get(key) {
        if (!objects.has(key)) return null;
        return { size: Buffer.byteLength(objects.get(key)), async text() { return objects.get(key); } };
      },
    },
  };
  async function call(operation, args, tenant = 'one', token = env.TOOL_API_KEY) {
    return worker.fetch(new Request('https://tools.example.com/' + operation, { method:'POST', headers:{authorization:'Bearer '+token}, body:JSON.stringify({tenant_id:tenant,session_id:'s',call_id:'c',arguments:args}) }), env);
  }
  assert.equal((await call('write',{key:'note',text:'hello'},'one','wrong')).status,401);
  assert.equal((await (await call('write',{key:'note',text:'hello'})).json()).is_error,false);
  assert.match(JSON.stringify(await (await call('read',{key:'note'})).json()),/hello/);
  assert.equal((await (await call('read',{key:'note'},'two')).json()).is_error,true);
  assert.equal((await (await call('read',{key:'../note'})).json()).is_error,true);
  env.ALLOW_WRITE='false';
  assert.equal((await (await call('write',{key:'note',text:'overwrite'})).json()).is_error,true);
  assert.equal((await call('write',{key:'note',text:'x'.repeat(70000)})).status,413);
});
