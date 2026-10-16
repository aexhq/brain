import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { appendFile, copyFile, mkdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { missingArtifacts, waitForArtifacts } from './registry-artifacts.mjs';

const [registry, operation] = process.argv.slice(2);
assert.ok(['publish', 'prepare', 'verify'].includes(operation), 'unknown release operation');
const directory = path.resolve('dist/sdk');
const release = JSON.parse(await readFile(path.join(directory, 'release.json'), 'utf8'));
assert.equal(release.commit, process.env.RELEASE_COMMIT, 'artifact does not belong to the verified commit');
const missing = await missingArtifacts(registry, release, directory);
if (operation === 'prepare') {
  assert.equal(registry, 'pypi');
  const upload = path.resolve('dist/pypi-upload');
  await mkdir(upload, { recursive: true });
  for (const file of missing) await copyFile(path.join(directory, file), path.join(upload, file));
  await appendFile(process.env.GITHUB_OUTPUT, `publish=${missing.length > 0}\n`);
} else {
  if (operation === 'publish' && missing.length) {
    assert.equal(registry, 'npm');
    const result = spawnSync('npm', ['publish', path.join(directory, release.npm), '--access', 'public', '--provenance', '--ignore-scripts'], { stdio: 'inherit' });
    if (result.error) throw result.error;
    assert.equal(result.status, 0, 'npm publication failed');
  }
  if (missing.length) await waitForArtifacts(registry, release, directory);
}
const summary = `${registry}: SDK ${release.version}, server ghcr.io/aexhq/brain:sha-${release.commit}, ${operation} complete.\n`;
console.log(summary);
if (process.env.GITHUB_STEP_SUMMARY) await appendFile(process.env.GITHUB_STEP_SUMMARY, summary);
