import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { missingArtifacts } from '../scripts/registry-artifacts.mjs';
import { validateRelease } from '../scripts/validate-sdk-release.mjs';

test('release admission rejects PR runs, another source, and failed verification or deployment', () => {
  const commit = 'a'.repeat(40), repository = 'aexhq/brain', ref = `refs/tags/release/sha-${commit}`;
  const run = { repository: { full_name: repository }, head_sha: commit, head_branch: 'main', event: 'push', path: '.github/workflows/ci.yml' };
  const jobs = ['build-test', 'publish', 'deploy-docs'].map(name => ({ name, conclusion: 'success' }));
  validateRelease(run, jobs, repository, commit, ref);
  assert.throws(() => validateRelease({ ...run, event: 'pull_request' }, jobs, repository, commit, ref));
  assert.throws(() => validateRelease({ ...run, head_sha: 'b'.repeat(40) }, jobs, repository, commit, ref));
  assert.throws(() => validateRelease(run, jobs, repository, commit, 'refs/heads/main'));
  for (const job of jobs) {
    assert.throws(() => validateRelease(run, jobs.map(item => item === job ? { ...item, conclusion: 'failure' } : item), repository, commit, ref));
  }
});

test('publication distinguishes missing versions, immutable matches, partial uploads, outages and version reuse', async t => {
  const directory = await mkdtemp(path.join(tmpdir(), 'brain-registry-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const release = { version: '1.0.0', npm: 'sdk.tgz', python: ['sdk.whl', 'sdk.tar.gz'] };
  for (const file of [release.npm, ...release.python]) await writeFile(path.join(directory, file), 'tested bytes');
  for (const registry of ['npm', 'pypi']) {
    const expected = registry === 'npm' ? [release.npm] : release.python;
    assert.deepEqual(await missingArtifacts(registry, release, directory, async () => new Response('', { status: 404 })), expected);
    await assert.rejects(missingArtifacts(registry, release, directory, async () => new Response('', { status: 503 })), /HTTP 503/);
    const metadata = registry === 'npm' ? { dist: { tarball: 'https://files.test/sdk' } } : { urls: expected.map(filename => ({ filename, url: 'https://files.test/' + filename })) };
    const request = bytes => async url => url.startsWith('https://files.test/') ? new Response(bytes) : Response.json(metadata);
    assert.deepEqual(await missingArtifacts(registry, release, directory, request('tested bytes')), []);
    await assert.rejects(missingArtifacts(registry, release, directory, request('different bytes')), /bump both SDK versions/);
  }
  assert.deepEqual(await missingArtifacts('pypi', release, directory, async url => url.startsWith('https://files.test/') ? new Response('tested bytes') : Response.json({ urls: [{ filename: 'sdk.whl', url: 'https://files.test/sdk.whl' }] })), ['sdk.tar.gz']);
});
