import assert from 'node:assert/strict';
import { readFile, readdir, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const site = fileURLToPath(new URL('../', import.meta.url));
const out = path.join(site, 'out');
const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? '';
const read = file => readFile(path.join(out, file), 'utf8');
const documentFiles = (await readdir(path.join(out, 'docs'), { recursive: true }))
  .filter(file => file.endsWith('index.html')).map(file => path.join('docs', file));
const contractText = await readFile(path.join(site, '../api/openapi.json'), 'utf8');
const contract = JSON.parse(contractText);
const operations = Object.entries(contract.paths).flatMap(([route, methods]) =>
  Object.entries(methods).map(([method, operation]) => ({ route, method, ...operation })));

async function resolveFile(url) {
  assert.ok(url.pathname.startsWith(basePath + '/'), `URL escapes deployment base: ${url.pathname}`);
  const name = decodeURIComponent(url.pathname.slice(basePath.length)).replace(/^\//, '');
  const direct = path.join(out, name);
  try { if ((await stat(direct)).isFile()) return direct; }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  const index = path.join(direct, 'index.html');
  assert.ok((await stat(index)).isFile(), `Missing static output for ${url.pathname}`);
  return index;
}

test('every rendered documentation link and anchor resolves in the static export', async () => {
  assert.ok(documentFiles.length > operations.length, 'Authored guides must be published alongside the API');
  const failures = [];
  for (const file of documentFiles) {
    const html = await read(file);
    const url = new URL(basePath + '/' + file.replaceAll(path.sep, '/').replace(/index\.html$/, ''), 'https://brain.test');
    for (const [, href] of html.matchAll(/<a\b[^>]*\bhref="([^"]+)"/g)) {
      const target = new URL(href.replaceAll('&amp;', '&'), url);
      if (target.origin !== url.origin) continue;
      try {
        const targetFile = await resolveFile(target);
        if (target.hash && targetFile.endsWith('.html')) {
          const body = targetFile === path.join(out, file) ? html : await readFile(targetFile, 'utf8');
          assert.ok(body.includes(`id="${decodeURIComponent(target.hash.slice(1))}"`), `Missing anchor ${target.hash}`);
        }
      } catch (error) { failures.push(`${url.pathname} → ${href}: ${error.message}`); }
    }
  }
  assert.deepEqual(failures, []);
});

test('every operation publishes concrete responses, readable HTML, and complete Markdown', async () => {
  assert.equal(await read('openapi.json'), contractText, 'Published OpenAPI must match the Go-generated artifact');
  for (const operation of operations) {
    const prefix = `api/generated/${operation.operationId}`;
    const html = await read(`docs/${prefix}/index.html`);
    assert.ok(html.includes(operation.summary), `Missing ${operation.operationId} title`);
    assert.ok(html.includes(operation.route), `Missing ${operation.operationId} endpoint`);
    assert.ok(html.replace(/<[^>]+>/g, '').includes('Authorization: Bearer'), `Missing authentication in ${operation.operationId} request example`);
    const md = await read(`llms.mdx/docs/${prefix}/content.md`);
    assert.ok(md.includes(`${operation.method.toUpperCase()} ${operation.route}`));
    const match = md.match(/```json\n([\s\S]*?)\n```/);
    assert.ok(match, `Missing exported schema for ${operation.operationId}`);
    const exported = JSON.parse(match[1]);
    const published = exported.paths[operation.route][operation.method];
    assert.deepEqual(published, contract.paths[operation.route][operation.method]);
    assert.ok(Object.entries(published.responses).some(([code, response]) => /^2\d\d$/.test(code) && response.content['application/json'].schema));
    for (const [, name] of match[1].matchAll(/#\/components\/schemas\/([^"\s]+)/g)) {
      assert.ok(exported.components.schemas[name], `Unresolved schema ${name}`);
    }
  }
});

test('static search and agent-readable exports cover the published guides and API', async () => {
  const search = JSON.parse(await read('api/search'));
  const indexed = new Set(Object.values(search.docs.docs).filter(doc => doc.type === 'page').map(doc => doc.url));
  const index = await read('llms.txt');
  const full = await read('llms-full.txt');
  for (const file of documentFiles) {
    const url = '/' + file.replaceAll(path.sep, '/').replace(/\/index\.html$/, '');
    assert.ok(indexed.has(url), `Search misses ${url}`);
    assert.ok(index.includes(`](${basePath}${url})`), `Markdown index misses ${url}`);
    const md = await read(`llms.mdx/${url.slice(1)}/content.md`);
    assert.ok(md.length > 100, `Empty Markdown for ${url}`);
    assert.ok(full.includes(md.trim()), `Full export misses ${url}`);
  }
  assert.ok(full.includes('not live-provider qualification'));
  assert.ok(full.includes('type Factory interface'));
});

test('overview pages leave temporary gaps to the roadmap', async () => {
  const gap = /\b(?:fixed (?:at|on|upon|when)(?: you)? creat\w*|frozen|not supported|(?:isn|aren)['’]t supported|(?:doesn|don)['’]t support|not yet|(?:isn|aren)['’]t yet|for now|currently)\b/gi;
  const failures = [];
  for (const file of ['../README.md', 'content/docs/index.mdx', 'content/docs/architecture.mdx']) {
    for (const [phrase] of (await readFile(path.join(site, file), 'utf8')).matchAll(gap)) failures.push(`${file}: "${phrase}"`);
  }
  assert.deepEqual(failures, [], 'Describe what Brain does here; planned work belongs in docs/roadmap.md (see docs/writing.md)');
});
