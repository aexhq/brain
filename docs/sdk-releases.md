# SDK installation and delivery

JavaScript/TypeScript applications install `@aexhq/brain`. The client uses openapi-fetch and OpenTelemetry; the bundled CLI uses Commander.

Node applications that start their own Brain process install `brainserver`.
It packages the portable client and Node server entry point. npm selects the Brain
executable, Pi/Codex/OpenCode runners, and complete locked production dependency tree
through one of six optional
`@aexhq/brainserver-<platform>-<arch>` dependencies (Linux, macOS, and Windows;
x64 and arm64). No installation scripts or user-side compilation are needed.

```sh
npm install --save-exact @aexhq/brain@4.0.0
```

Release artifacts include the npm archive in [GitHub Releases](https://github.com/aexhq/brain/releases). The release workflow publishes those exact tested bytes to npm.

SDK 2.2.0 adds `AwsS3`, `Modal`, `E2B`, and `Daytona` helpers. The Node server installs these through a constructor's tool list; client sessions use the same helpers to select operations and optionally supply private credentials. Missing credentials fail explicitly and rejected overrides never fall back to server defaults. Client-owned runtime functions use the session binding contract. Successful output schemas validate `structured_content`. Update server, runner, and clients together; routes and local state have no compatibility layer or migration. Pin the SDK and the server image commit recorded in the publishing workflow summary.

Go applications import `github.com/aexhq/brain` and optionally `github.com/aexhq/brain/tools/integrations`. Pin the module to the source commit recorded in the matching release with `go get github.com/aexhq/brain@COMMIT`.

## Maintainer workflow

1. Change the version in `sdks/typescript/package.json` and refresh the npm lockfile. Use a major for breaking
   client contracts, minor for compatible additions, patch for fixes. Changes to
   packaged files require a new version; unrelated server changes do not.
2. Run `node scripts/setup.mjs` and `node scripts/check.mjs`. The full check packs
   the npm distribution and installs it in an isolated consumer.
   Tests include package exports/types and real-server journeys through installed
   the TypeScript client. All previous tests remain required.
3. Merge only when the protected CI gate and CodeQL pass. Linux CI uploads
   `sdk-packages`, including `release.json` with the source commit and filenames.
4. After the main push passes verification, image publication, and docs deployment,
   CI dispatches `npm-publish.yml` at `release/sha-<commit>`. This preserves the
   existing `npm-production` environment approval and tag restriction. The release
   validates the exact source and completed CI gates, then downloads that run's exact
   archives. It publishes with OIDC and verifies downloaded public bytes, then
   installs from each registry. It never rebuilds for publication. An existing
   version must contain identical bytes; a mismatch fails and requires a bump.
   Read-only verification polls for delayed registry visibility after an accepted
   upload; it never repeats a publish. The same archives are also attached to a
   GitHub release at the verified commit tag for direct downloads.

The same CI run publishes tested container images and deploys the docs site.
Approve the existing production environment gate, then re-run failed release jobs after correcting registry access. Package timestamps
are normalized for reproducibility; source identity lives in `release.json`.

## Registry setup

The workflow does not grant itself registry ownership. Configure these values
in the existing registry owner's account:

| Registry | Project | GitHub owner/repository | Workflow | Environment |
| --- | --- | --- | --- | --- |
| npm | `@aexhq/brain` | `aexhq/brain` | `npm-publish.yml` | None |
| npm | `brainserver` and the six `@aexhq/brainserver-*` platform packages | `aexhq/brain` | `npm-publish.yml` | None |

Use [npm trusted publishing](https://docs.npmjs.com/trusted-publishers/). Missing registry trust fails publication.

Claim the new server package names using the registry owner's account and configure
their trusted publisher before the first automated server release. CI publishes
platform archives before the main `brainserver` archive, verifies the public bytes,
then installs and starts the server from npm. Missing registry ownership or trust
fails publication. The existing SDK registry configuration does not grant access
to these new names.

## Building from source

Source builds remain available for contributors. `node scripts/package-sdks.mjs`
requires the TypeScript SDK to be built and creates all distributions in
`dist/sdk/`. It installs the package under `bin/sdk-consumer` and verifies its exports, types and CLI. Install an unpublished archive with `npm install /path/to/archive.tgz`.

`npm run build` builds the harness, client, and host executable in
one command, then packs `dist/server/`. Use `npm run build -- --all-platforms` to cross-compile all
six executable packages. `node --test tests/server-package.test.mjs` installs the
main archive and the host platform archive in an isolated consumer and tests the
installed server. The full check includes both commands; Linux CI uploads
`server-packages`, and the release workflow publishes those exact archives.
Change `packages/brainserver/package.json` when releasing a changed server package;
its platform packages share that version. The remote SDK versions are independent.
