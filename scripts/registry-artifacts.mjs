import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

// A 404 means unpublished. Auth failures, outages, and version reuse must fail.
export async function missingArtifacts(registry, release, directory, request = fetch) {
  assert.ok(['npm', 'pypi'].includes(registry), 'unknown registry');
  if (registry === 'npm' && release.npmPackages) {
    const missing = [];
    for (const { name, file } of release.npmPackages) {
      missing.push(...await missingArtifacts(registry, { version: release.version, npm: file, npmName: name }, directory, request));
    }
    return missing;
  }
  const url = registry === 'npm'
    ? `https://registry.npmjs.org/${encodeURIComponent(release.npmName ?? '@aexhq/brain').replaceAll('%40', '@')}/${release.version}`
    : `https://pypi.org/pypi/aex-brain/${release.version}/json`;
  const response = await request(url, { signal: AbortSignal.timeout(30000) });
  const files = registry === 'npm' ? [release.npm] : release.python;
  if (response.status === 404) return files;
  assert.equal(response.status, 200, `${registry} metadata: HTTP ${response.status}`);
  const metadata = await response.json();
  const published = registry === 'npm'
    ? [{ filename: release.npm, url: metadata.dist.tarball }]
    : metadata.urls;
  const missing = [];
  for (const file of files) {
    const remote = published.find(item => item.filename === file);
    if (!remote) { missing.push(file); continue; }
    const download = await request(remote.url, { signal: AbortSignal.timeout(30000) });
    assert.equal(download.status, 200, `${file}: HTTP ${download.status}`);
    assert.ok((await readFile(path.join(directory, file))).equals(Buffer.from(await download.arrayBuffer())),
      `${file} already exists with different bytes; bump both SDK versions or the server package version`);
  }
  return missing;
}

// Registries acknowledge uploads before public metadata becomes visible. Poll
// only reads; never repeat a publish. Outages and byte mismatches still fail fast.
export async function waitForArtifacts(registry, release, directory, { request = fetch, pause = delay, attempts = 20 } = {}) {
  for (let attempt = 0; attempt < attempts; attempt++) {
    if ((await missingArtifacts(registry, release, directory, request)).length === 0) return;
    if (attempt + 1 < attempts) await pause(15000);
  }
  throw new Error(`${registry} did not expose the tested distributions before the verification deadline`);
}
