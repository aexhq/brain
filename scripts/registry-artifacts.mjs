import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

// A 404 means unpublished. Auth failures, outages, and version reuse must fail.
export async function missingArtifacts(registry, release, directory, request = fetch) {
  assert.ok(['npm', 'pypi'].includes(registry), 'unknown registry');
  const url = registry === 'npm'
    ? `https://registry.npmjs.org/@aexhq%2Fbrain/${release.version}`
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
      `${file} already exists with different bytes; bump both SDK versions`);
  }
  return missing;
}
