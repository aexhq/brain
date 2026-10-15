# 09: Delivery, documentation, and platform examples

Status: Implementation authorized on 2026-10-15. Complements [integration expansion](08-integration-expansion.md), [verification](07-verification-and-release.md), and the [research](../research/integration-options.md).

This cut implements the CI aggregate gate, exact-image promotion, provenance, downloadable binaries, GitHub Pages subpath build, dependency update configuration, Docker/Fly/ECS recipes, an R2 Worker example, and TypeScript/Python session examples. Hosted fleet routing and a full application login/UI remain platform work, not Brain runtime entities. Cloud templates are labeled separately from live-qualified integrations.

## Delivery sequence

| Slice | Deliverable | Acceptance |
| --- | --- | --- |
| D1: Developer onboarding | Concise README, Bash/PowerShell quickstarts, runnable local stack. | A fresh checkout reaches a streaming session without undocumented steps. Deterministic test mode needs no paid account. |
| D2: Continuous verification | Existing full CI plus integration contracts, example checks, dependency updates. | No existing test is removed or skipped; generated artifacts and docs are checked. |
| D3: Artifacts and docs | GHCR image, downloadable matching build artifacts, deployed Fumadocs site. | Publication waits for all required checks and identifies the source commit; deployed URLs and image are smoke-tested. |
| D4: Application examples | TypeScript backend/UI flow, Python flow, external tools and sandbox examples. | Examples exercise actual Brain APIs and keep server credentials out of browsers. |
| D5: Platform recipes | Docker, Fly, AWS, and Cloudflare recipes; qualified subset published. | Start, stream, interrupt, restart/restore, and teardown work; unsupported fleet behavior is explicit. |

## CI/CD design

Retain GitHub Actions and the existing Linux/Windows, race, PostgreSQL, and container jobs. Prefer small workflow changes over a new monorepo/task/release framework.

The repository-owned CodeQL matrix scans Go, JavaScript/TypeScript, Python, and Actions and participates in the aggregate gate. It replaces the legacy default scan that targeted Rust. Weekly CI retains recurring security analysis; publication still runs only for verified pushes to `main`.

1. Keep `scripts/check.mjs` as the local full-check entry point. Add new provider-free contracts, package checks, example validation, and docs checks there or in clearly required CI jobs.
2. Pin actions and container base images for reproducibility. Use Dependabot for Go, npm, Docker, Actions, and new Python manifests. Updates still run all conformance tests; do not auto-merge harness changes based only on compilation.
3. Add credentialed qualification jobs for each advertised cloud integration. Use trusted code on protected branches, AWS OIDC where available, and scoped secrets for other providers. Never run untrusted PR code with provider credentials.
4. A selected qualification profile fails if credentials are missing. Ordinary PR CI remains credential-free; it cannot confer a live-qualified status. Release gates require successful qualification of the exact candidate for features advertised as qualified.
5. Build the container once, smoke-test that artifact, and publish/promote the same artifact. Avoid rebuilding untested bytes in the deploy job. Use the commit SHA as the image tag and record the digest.
6. Gate publishing on successful checks for the exact commit, including PostgreSQL/race/container jobs. Do not allow a docs deployment or image push to race ahead of a failing required job.
7. Publish build provenance using GitHub's supported actions and job-scoped permissions. Test artifact extraction and startup; include the selected runner and its production dependencies, not only the Go binary.
8. Keep platform deployment separate from core verification. A failed cloud deploy cannot be presented as a successful release. Verify health/readiness, a session journey, and the externally served docs after deployment.

No semantic API versions, migration compatibility, or old runner negotiation are introduced. Commit IDs and OCI digests identify build artifacts. npm/PyPI publication needs registry version identifiers, so leave SDK registry publication out of this cut unless that policy is explicitly changed; source and matching downloadable artifacts remain usable.

Linux amd64 is the first published container target because it already has a container journey. Add arm64 and macOS artifacts only with native runtime checks for their harness dependencies. A successful cross-compile alone is insufficient.

Do not add manual human approval steps to routine verified releases. Existing repository/environment protections still apply; if they prevent the authorized merge/deploy, report the exact restriction and leave reviewable artifacts.

## Documentation deployment

Use the existing static export. Prefer GitHub Pages initially because the repository already uses GitHub; use Cloudflare Pages instead if the existing account/domain setup makes it simpler. Do not introduce a running Next.js server for static documentation.

For a GitHub repository site, configure `/brain` at build time and verify every root-relative reference: header links, search index, OpenAPI download, Markdown exports, `llms.txt`, assets, and copied URLs. Keep local development at the root path. The deployed smoke test must fetch those files, not only the homepage.

Publish docs from the same verified source commit as the described artifacts. Add real source/edit links and a truthful support matrix. Keep experimental integrations identifiable at their own setup pages rather than burying caveats on a separate roadmap.

## README refinements

Keep the README short and task-oriented:

1. One paragraph explaining what Brain does and who embeds it.
2. Current support, clearly separated from planned integrations.
3. A working local quickstart with Bash and PowerShell paths, plus Docker when verified.
4. A short create/send/stream example and links to the SDK guides.
5. Links to tool setup, persistence/recovery, platform examples, and the hosted docs.
6. A contribution/check command, CI status, license, and the clean-break policy.

Reuse runnable files for long examples. Keep architecture detail and provider option lists in the site. Preserve existing documentation refinements while correcting any claims that exceed test evidence.

## Fumadocs refinements

Continue the current Brain visual design. Reuse installed Tabs, Steps, Callout, and code components; no new design system is needed.

| Section | Add or refine | Source of truth |
| --- | --- | --- |
| Getting started | Bash/PowerShell, local deterministic demo, real provider, Docker | Checked scripts/config files under `examples/` |
| Sessions | Queue/interrupt/resume, SSE reconnection, tool waits, accepted vs completed input | Actual API contract and journey behavior |
| Harnesses | Setup, provider/auth subset, native state, isolation and recovery status | Runner code plus conformance results |
| Tools | HTTP/SSE authoring, MCP, storage, web, sandboxes, approvals, browser | Service descriptors and runnable examples |
| Persistence | S3/R2/GCS, PostgreSQL/Neon/Supabase, restore drills, retention limits | Adapter settings and tested examples |
| Deploy | Local/Docker, Fly, AWS, Cloudflare; Cloud Run limitations | Checked platform recipes and qualification results |
| API/SDK | Complete response schemas, errors, generated types, practical snippets | Go-owned contract and SDK sources |
| Operations | Required resources, overload, shutdown, backup/restore, telemetry | Metrics/config code and measured reports |

Extend `website/scripts/generate.mjs` to import tested snippets or descriptors when needed. Avoid a parallel hand-maintained integration manifest unless it replaces real duplication. Keep existing link/anchor, API, search, and Markdown-export tests. Add browser checks only for interactions new static tests cannot verify, such as deployed search or streamed application UI behavior.

## Application examples

Every example includes prerequisites, an exact start command, expected behavior, error handling, and teardown. No sample credentials should look usable. Provider-free tests exercise the same integration boundary as the live example.

| Example | Complete journey | Boundary to demonstrate |
| --- | --- | --- |
| TypeScript server | Create session, send, stream, reconnect, interrupt, resume | Backend owns the Brain token; client sees only authorized session data. |
| Next.js chat | Thin server route forwards authorized messages and streaming events; UI reconnects | Do not expose a global management token or rely on client-supplied tenant IDs. Avoid inventing a user-account product. |
| Python application | Same lifecycle using the existing Python client | Cancellation of an observer differs from interruption of an agent. |
| Storage/research agent | Search, fetch, save an artifact to a scoped bucket | Source attribution, bounded fetches, and tool errors remain model-visible. |
| Sandbox coding agent | Create isolated sandbox, run code, inspect output, terminate | API keys and provider ownership remain in the external service. |
| Approval | Hold a tool call or return a request handle; operator approves/denies | The action tool enforces approval; Brain does not have an approval API. |
| Cold recovery | Flush on A, stop A, restore into empty B, inspect, explicitly resume | Same native model context; no action replay or inference during restoration. |

Prefer one small UI example and one Python example over duplicating every framework. Extend the existing `examples/session.mjs` and approval service where they already fit.

## Platform recipes

### Local process and Docker Compose

Provide a small default stack: Brain with Pi and one external tool service. Optional profiles add PostgreSQL and an OpenTelemetry collector. Keep cloud archive configuration optional. Use a recording provider for automated onboarding tests, clearly labelled as a fixture rather than an LLM.

Test health/readiness, first session, streamed tool output, interruption, local restart, and clean teardown. Preserve volumes unless the operator explicitly chooses to delete data.

### Fly

Provide `fly.toml`, secrets setup, archive/index configuration, and smoke/teardown commands. Begin with one machine and a mounted local cache; disable request-idleness-based autostop for active background work. Demonstrate planned stop/flush/restore before adding replicas.

Multiple machines require an external session-owner mapping and routing before admission. `fly-replay` is a routing primitive, not an ownership lock. Do not infer shared state from volumes or present an ordinary round-robin deployment as safe.

### AWS

Provide an ECS Fargate task/service recipe using task-role S3 access, external PostgreSQL, disposable local storage, private tool endpoints, and an ingress that supports SSE. Keep region, bucket, network, and credentials operator supplied.

Start with one task. During replacement, prevent simultaneous ownership: stop/drain the old task before making the new owner active. Match container termination grace and ingress drain behavior to Brain's measured shutdown path. A replicated service needs platform routing and fencing outside Brain.

### Cloudflare

First provide a Worker-hosted R2 tool using native bindings, showing that tools can run anywhere. Brain can use the S3-compatible R2 archive independently.

For Brain hosting, use Containers with explicit Durable Object/platform routing. The Go binary and child harness processes do not run in a normal Worker. Test idle container termination, recreation, outbound model access, streaming, and restoring native state. Do not move session execution into a new Durable Object implementation of Brain.

### Cloud Run and other serverless hosts

Keep these as experimental recipes until ownership and background CPU behavior are demonstrated. Cloud Run needs instance-based CPU allocation for work after `202`, reconnectable streams, and explicit placement beyond best-effort affinity. Its memory-backed writable filesystem also needs conservative cache limits.

Do not add cloud task queues or a hosted scheduler to make a request-scoped function appear to be Brain. If the host cannot keep the authorized turn running, use it for tools or the application's frontend/backend instead.

## Verification and rollout

Run the full existing suite after implementation and require all CI jobs before merge. Add provider-free integration tests to ordinary CI and live tests to selected qualification profiles. Record what was run, which provider accounts were used by name only, and any unqualified features.

Merge the verified branch, publish the exact image/docs artifacts, and smoke-test their public destinations. Keep cloud resources limited to the requested development deployment; clean up temporary sandboxes and test objects. Retain only the intentionally deployed service and required state. Never commit browser-derived API keys or display them in logs.

If a deployment account is unavailable, complete the runnable recipe and release artifacts, record the specific blocker, and continue other authorized integrations. Do not claim a provider or deployment passed without executing its required checks.
