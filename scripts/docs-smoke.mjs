// Verifies the deployed export, including assets that can break under a base path.
import assert from 'node:assert/strict';

const base = new URL(process.argv[2]);
assert.ok(base.pathname.endsWith('/'), 'provide the site URL with a trailing slash');
const checks = [
  ...['docs/', 'docs/api/', 'docs/roadmap/', 'docs/tools/sandboxes/', 'docs/tools/resources/', 'docs/guides/platforms/'].map(route =>
    [route, body => { assert.match(body, /<html\b/i); assert.ok(body.includes('Brain')); }]),
  ['openapi.json', body => assert.ok(JSON.parse(body).paths['/sessions'])],
  ['api/search', body => assert.ok(Object.keys(JSON.parse(body).docs.docs).length > 0)],
  ['llms.txt', body => assert.ok(body.includes(`](${base.pathname}docs/quickstart)`))],
  ['llms-full.txt', body => assert.ok(body.includes('type Factory interface'))],
  ['llms.mdx/docs/quickstart/content.md', body => assert.ok(body.startsWith('# Quickstart'))],
];

for (const [route, check] of checks) {
  const url = new URL(route, base);
  const response = await fetch(url, { signal: AbortSignal.timeout(15000) });
  assert.equal(response.status, 200, `${url}: HTTP ${response.status}`);
  check(await response.text());
  console.log(`OK ${url}`);
}
