import { randomBytes } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
const target = new URL('../deploy/docker/.env', import.meta.url);
let existing = '';
try { existing = await readFile(target, 'utf8'); } catch (error) { if (error.code !== 'ENOENT') throw error; }
const names = new Set(existing.split(/\r?\n/).map(line => line.split('=')[0]));
let additions = '';
for (const name of ['BRAIN_API_KEY', 'BRAIN_SERVICE_TOKEN', 'BRAIN_ENCRYPTION_KEY']) {
  if (!names.has(name)) additions += `${name}=${randomBytes(32).toString(name === 'BRAIN_ENCRYPTION_KEY' ? 'base64' : 'hex')}\n`;
}
if (additions) await writeFile(target, existing + (existing && !existing.endsWith('\n') ? '\n' : '') + additions, { mode: 0o600 });
console.log('Local Docker credentials are ready in deploy/docker/.env.');
