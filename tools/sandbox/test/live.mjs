import assert from 'node:assert/strict';
import { sandboxService } from '../src/service.mjs';
import { providersFromEnv } from '../src/providers.mjs';
import { randomBytes } from 'node:crypto';

const provider = process.argv[2];
const providers = providersFromEnv();
if (!provider || !Object.hasOwn(providers, provider)) throw new Error('name a provider with configured credentials; live tests never skip');
const invoke = sandboxService({ providers, signingKey: randomBytes(32).toString('hex') });
const signal = AbortSignal.timeout(240000);
async function call(path, args) {
  const out = await invoke(path, { tenant_id: 'qualification', session_id: 'qualification', call_id: path, arguments: args }, signal);
  assert.ok(!out.is_error, JSON.stringify(out));
  return JSON.parse(out.content[0].text);
}
const { sandbox } = await call('/create', { provider });
try {
  const output = await call('/exec', { sandbox, command: 'printf brain-sandbox-ok' });
  assert.equal(output.stdout, 'brain-sandbox-ok'); assert.equal(output.exit_code, 0);
  await call('/inspect', { sandbox });
  console.log(`${provider}: create, exec, inspect passed`);
} finally { await call('/terminate', { sandbox }); }
console.log(`${provider}: termination passed`);
process.exit(0);
