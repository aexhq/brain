# 10: Published clients and subsystem boundaries

Status: implemented and locally verified; registry publication and rollout pending. Authorized 2026-10-16.

## Findings and decisions

The Go runtime already has generated HTTP contracts, isolated harnesses, bounded
execution, archive adapters, and CI-gated image/docs delivery. Its remaining
coupling is concrete: session coordination imports the tool registry, local SQL
metadata shares a type with filesystem layout, and tool policy and transport live
in one implementation. SDK tests import checkout files, so they cannot detect an
uninstallable package. The npm name `@aexhq/brain` already contains the predecessor
runtime's SDK (0.41.0); the replacement needs a deliberate major release.

Follow [ADR 0002](../adr/0002-subsystem-contracts-and-sdk-delivery.md). Preserve
ADR 0001's single owner, local durability, passive recovery, and error boundaries.
This work does not turn the PostgreSQL discovery index into a session lock or
introduce a hosted control plane.

## Implementation sequence and acceptance

1. **Subsystem contracts.** Put the interfaces used by session coordination in
   `internal/session`; isolate tool policy from HTTP/SSE and MCP adapters; separate
   SQL metadata from local files. Keep the existing behavior and add contract tests
   for tenant isolation, failed writes, cancellation, transport substitution, and
   restart persistence. Extend generated adapter documentation from actual code.
2. **Installable SDKs.** Ship `@aexhq/brain` and `aex-brain` (Python import `brain`)
   at 1.0.0, including license, README, generated types, and Python `py.typed`.
   Install packed npm/wheel/sdist artifacts into isolated consumer directories;
   run SDK tests and existing real-server journeys through installed clients.
3. **Verified delivery.** Add package checks to the full Linux/Windows suite and
   upload the tested Linux archives. Publish those archives using registry OIDC
   only after the existing aggregate CI gate. Verify public downloads against
   those archives; unchanged versions must match or fail, never silently overwrite.
   Retain image/docs checks and deployment.
4. **Consumer onboarding.** Replace checkout imports and PYTHONPATH instructions
   with registry installation. Document the major break from the predecessor,
   exact SDK/server pinning, extension contracts, and registry setup requirements.
5. **Rollout.** Run every local check, real PostgreSQL and container checks; push a
   PR, wait for every required CI job, merge, and verify packages, GHCR and Pages.
   Record an unavailable registry/account as a blocker rather than a passing deploy.

## Research

- [Go interface guidance](https://go.dev/wiki/CodeReviewComments#interfaces):
  consumer-owned interfaces describe actual dependencies; constructors return
  concrete implementations. No universal plugin registry or speculative CRUD API.
- [npm trusted publishing](https://docs.npmjs.com/trusted-publishers/): GitHub
  OIDC removes stored publishing tokens, requires a matching workflow identity and
  repository metadata, and supplies provenance. Reuse `npm-publish.yml`, the
  predecessor's publishing workflow identity.
- [Python packaging](https://packaging.python.org/en/latest/tutorials/packaging-projects/):
  use standard pyproject metadata, a pinned PEP 517 backend, wheel and sdist.
- [PyPI pending publishers](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/)
  and [publishing](https://docs.pypi.org/trusted-publishers/using-a-publisher/):
  a new project needs an account-side pending publisher before its first OIDC
  upload. A workflow file alone cannot grant registry ownership.

## Checks with a concrete failure consequence

Package installation catches missing exports/types/license or checkout-only
imports and blocks CI. Common tool output validation prevents a new transport
from bypassing result/size policy and returns a tool error. Adapter contract tests
catch tenant-crossing writes and database failures before release. Comparing an
already-published immutable artifact catches accidental version reuse and stops
publication. No additional runtime negotiation, retry layer, or migration system
is required.

## Verification evidence

The full local `check.mjs` passed on Windows, including generated contracts, all
Go/Pi/service/client tests, installed-package journeys, and docs build/link/export
checks. A real PostgreSQL 18 contract run and both Docker image smoke tests passed.
Workflow syntax passes actionlint. Registry admission tests reject PR runs,
different source commits, failed CI/deployment gates, registry outages, and
immutable version collisions. Linux/race/CodeQL remain required in PR CI.

The existing `npm-production` environment only accepts `release/sha-*` tags and
requires its configured reviewer. CI therefore dispatches SDK publication at a
commit tag after images and docs succeed. This preserves that protection and
keeps provenance tied to the verified source even if main advances. The first
PyPI upload needs account-side pending-publisher setup; it is not counted as passed.
