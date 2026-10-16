import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function validateRelease(run, jobs, repository, commit, ref) {
  assert.match(commit, /^[0-9a-f]{40}$/);
  assert.equal(ref, `refs/tags/release/sha-${commit}`, 'release must use its immutable source tag');
  assert.equal(run.repository.full_name, repository);
  assert.equal(run.head_sha, commit, 'CI ran a different commit');
  assert.equal(run.head_branch, 'main');
  assert.equal(run.event, 'push', 'only protected main push CI can release');
  assert.equal(run.path, '.github/workflows/ci.yml');
  // The dispatcher itself may still be completing; all verification and
  // deployment dependencies must already have succeeded for this exact run.
  for (const name of ['build-test', 'publish', 'deploy-docs']) {
    assert.equal(jobs.find(job => job.name === name)?.conclusion, 'success', `${name} has not passed`);
  }
}

if (path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { CI_RUN, GITHUB_REPOSITORY, GITHUB_SHA, GITHUB_REF, GH_TOKEN } = process.env;
  assert.match(CI_RUN, /^\d+$/);
  async function api(route) {
    const response = await fetch(`https://api.github.com/repos/${GITHUB_REPOSITORY}/actions/runs/${CI_RUN}${route}`, {
      headers: { Authorization: `Bearer ${GH_TOKEN}`, Accept: 'application/vnd.github+json' }, signal: AbortSignal.timeout(30000),
    });
    assert.equal(response.status, 200, `CI verification: HTTP ${response.status}`);
    return response.json();
  }
  const [run, jobs] = await Promise.all([api(''), api('/jobs?per_page=100')]);
  validateRelease(run, jobs.jobs, GITHUB_REPOSITORY, GITHUB_SHA, GITHUB_REF);
  console.log(`Verified release source ${GITHUB_SHA} against main CI run ${CI_RUN}`);
}
