# SDK installation and delivery

JavaScript/TypeScript applications install `@aexhq/brain`; Python applications
install `aex-brain` and import `brain`. Both are dependency-free at runtime.

The first Python distribution is available as a tested GitHub release wheel.
PyPI rejected the first upload with `invalid-publisher`; account-side setup below
is still required before the PyPI install command works. Install the released
wheel without building anything locally:

```sh
python -m pip install https://github.com/aexhq/brain/releases/download/release/sha-67afdc3d6403d101af22d3bce3dd8c6c79cb4557/aex_brain-1.0.0-py3-none-any.whl
```

```sh
npm install --save-exact @aexhq/brain@1.0.0
python -m pip install aex-brain==1.0.0
```

Version 1.0.0 is the client for the current Go session server. The predecessor's
npm 0.x releases expose a different API and cannot be used with it. There is no
automatic migration: update applications to create/send/observe sessions using
the current SDK guides. Server routes and local state remain unversioned; pin
the SDK and the server image commit recorded in the publishing workflow summary.

## Maintainer workflow

1. Change both package versions together in `sdks/typescript/package.json` and
   `sdks/python/pyproject.toml`; refresh the npm lockfile. Use a major for breaking
   client contracts, minor for compatible additions, patch for fixes. Changes to
   packaged files require a new version; unrelated server changes do not.
2. Run `node scripts/setup.mjs` and `node scripts/check.mjs`. The full check packs
   npm, wheel, and sdist distributions and installs them in isolated environments.
   Tests include package exports/types and real-server journeys through installed
   TypeScript and Python clients. All previous tests remain required.
3. Merge only when the protected CI gate and CodeQL pass. Linux CI uploads
   `sdk-packages`, including `release.json` with the source commit and filenames.
4. After the main push passes verification, image publication, and docs deployment,
   CI dispatches `npm-publish.yml` at `release/sha-<commit>`. This preserves the
   existing `npm-production` environment approval and tag restriction. The release
   validates the exact source and completed CI gates, then downloads that run's exact
   archives. It publishes with OIDC and verifies downloaded public bytes, then
   installs from each registry. It never rebuilds for publication. An existing
   version must contain identical bytes; a mismatch fails and requires a bump.
   Partial Python uploads can resume by publishing only missing files.
   Read-only verification polls for delayed registry visibility after an accepted
   upload; it never repeats a publish. The same archives are also attached to a
   GitHub release at the verified commit tag for direct downloads.

The same CI run publishes tested container images and deploys the docs site.
The SDK workflow has independent npm and PyPI jobs: one registry outage does not
undo the other's publication, and an incomplete workflow is not a complete release.
Approve the existing production environment gate, then re-run failed release jobs after correcting registry access. Package timestamps
are normalized for reproducibility; source identity lives in `release.json`.

## Registry setup

The workflow does not grant itself registry ownership. Configure these values
in the existing registry owner's account:

| Registry | Project | GitHub owner/repository | Workflow | Environment |
| --- | --- | --- | --- | --- |
| npm | `@aexhq/brain` | `aexhq/brain` | `npm-publish.yml` | None |
| PyPI | `aex-brain` | `aexhq/brain` | `npm-publish.yml` | `pypi` |

npm uses the predecessor's workflow identity. For a new Python project, create
a pending publisher on the PyPI account's Publishing page before the first upload.
PyPI consumes it on the first successful publication. No long-lived publishing
token is needed. Missing trust fails the job rather than silently skipping it.

See [npm trusted publishing](https://docs.npmjs.com/trusted-publishers/) and
[PyPI pending publishers](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/).

## Building from source

Source builds remain available for contributors. `node scripts/package-sdks.mjs`
requires the TypeScript SDK to be built and creates all distributions in
`dist/sdk/`. It installs pinned Python build tools in `bin/sdk-build` and consumer
environments under `bin/sdk-*`. No generated package, virtual environment, or
credential is committed. To use an unpublished change, install the resulting
archive with `npm install /path/to/archive.tgz` or `python -m pip install /path/to/package.whl`.
