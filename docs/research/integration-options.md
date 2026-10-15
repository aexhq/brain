# Integration research

Reviewed: 2026-10-15. These are recommendations, not newly implemented features or live-provider test results. The implementation baseline is commit `44e329d`; the existing documentation refinements in the working tree were also reviewed.

Start with the [integration implementation plan](../plans/08-integration-expansion.md) and [delivery, documentation, and examples plan](../plans/09-delivery-and-examples.md). They extend the existing R0–R4 roadmap rather than introduce another release system.

## What the code already provides

| Area | Evidence in this repository | Remaining work |
| --- | --- | --- |
| Runtime | `internal/session`, `internal/store`, `internal/api` | Capacity measurements and deployment qualification; a fleet router is outside Brain. |
| Cloud archives | `internal/persistence/objects.go`, `archive.go` | Filesystem, S3, and R2 are implemented. S3/R2 transport tests use a local HTTP fixture, not real cloud services. |
| SQL metadata | `internal/persistence/metadata.go` | PostgreSQL discovery is implemented and has a real PostgreSQL CI job. Hosted-provider connection recipes are unqualified. |
| Harness | `runners/pi`, `internal/harness` | Pi is pinned at `1.2.0`; OpenCode and Codex runners do not exist yet. |
| Tools | `internal/tools/http.go`, `examples/tools` | HTTP/SSE, inspection, and approval examples exist. MCP and provider catalog services do not. |
| Delivery | `.github/workflows/ci.yml`, `Dockerfile` | Linux/Windows checks, race detection, PostgreSQL, and Docker smoke tests exist. There is no image publication or docs deployment workflow. |
| Documentation | `website`, `cmd/generate` | Fumadocs, generated OpenAPI/types/interfaces/models, and site integrity tests exist. Platform recipes and application examples remain. |
| Distribution | `sdks/typescript/package.json`, `sdks/python` | TypeScript is private; Python is source-only without a packaging manifest. Do not advertise registry installation yet. |

## Storage: qualify existing adapters before adding another

**AWS S3 and Cloudflare R2 remain the initial cloud targets.** Both have conditional create semantics suitable for immutable checkpoint blocks. R2 documents its own compatibility surface, so the shared SDK does not remove the need for separate provider tests. Keep the current block archive; no provider-specific remote append API is needed. [S3 conditional writes](https://docs.aws.amazon.com/AmazonS3/latest/userguide/conditional-writes.html), [R2 conditional operations](https://developers.cloudflare.com/r2/api/s3/extensions/), [R2 S3 compatibility](https://developers.cloudflare.com/r2/api/s3/api/).

**Google Cloud Storage is the preferred third cloud adapter when there is demand.** Its generation-match precondition of zero supports create-if-absent. Use the official Go client and the existing `Objects` boundary. This is a fit recommendation, not a cost or latency comparison. A separate authenticated client adds a dependency, so defer it until S3/R2 qualification and a GCP use case justify it. [GCS preconditions](https://docs.cloud.google.com/storage/docs/request-preconditions).

**Azure Blob is a viable alternative to GCS, not a fourth launch commitment.** Its Go client supports block-blob uploads. Brain's chunk format works without choosing Azure Append Blobs; selecting a provider should not select a different transcript format. [Azure Go upload guide](https://learn.microsoft.com/en-us/azure/storage/blobs/storage-blob-upload-go).

**Neon and Supabase should initially be PostgreSQL recipes.** Both expose PostgreSQL connections. Their pooling behavior differs: Neon documents protocol-level prepared-statement support, while Supabase transaction pooling documents restrictions. Start with direct or session-pooled connections, verify TLS and the actual `pgx` queries, and qualify transaction pooling separately. Do not add a generic pooler workaround or a new SQL abstraction without a failing supported configuration. [Neon pooling](https://neon.com/docs/connect/connection-pooling), [Supabase connections](https://supabase.com/docs/guides/database/connecting-to-postgres).

Defer MySQL, D1, libSQL, DynamoDB, and Redis adapters until a concrete customer workflow needs them. SQLite is the local working database; PostgreSQL is an eventual discovery index. Replacing local metadata with a remote database would be a separate architectural decision.

## Harnesses: integration surfaces and unresolved gates

| Harness | Current evidence | Recommendation |
| --- | --- | --- |
| Pi | The installed SDK owns native session context, custom tools, abort, and steering. Brain already excludes ambient resources and turns off cache warming. | Keep as the reference adapter and shared-conformance baseline. Use the pinned installed source when checking behavior. |
| OpenCode v2 | An embedded `@opencode/sdk` host and plugin tool transforms are documented. Tool executors receive cancellation signals; request tool snapshots survive later registry changes. | Spike a TypeScript child runner embedding the SDK. Keep OpenCode's entities inside the runner. Do not implement both v1 and v2. |
| Codex | App Server exposes stdio, threads, streamed turns, interruption, and steering. Dynamic tool callbacks are experimental. | Spike a thin stdio bridge using native dynamic tool callbacks. Verify tool exclusion and portable native state before committing to support. |

Sources: [Pi SDK](https://raw.githubusercontent.com/badlogic/pi-mono/main/packages/coding-agent/docs/sdk.md), [OpenCode embedded SDK](https://opencode.ai/v2/docs/build/sdk/), [OpenCode plugin tools](https://opencode.ai/v2/docs/build/plugins/#tools), [Codex App Server](https://developers.openai.com/codex/app-server).

Two gates are particularly important:

1. **OpenCode startup recovery conflicts with passive restore.** Its current session contract describes startup recovery of claimed sessions and an added continuation instruction. `resume: false` on input admission does not establish that startup recovery is disabled. Prove an upstream control or maintain a narrow patch; otherwise the adapter stays experimental. OpenCode warming must separately remain disabled. [Session contract](https://raw.githubusercontent.com/anomalyco/opencode/dev/specs/v2/session.md), [warming configuration](https://opencode.ai/v2/docs/warming).
2. **Codex tool binding is not proof of exclusive tool access.** Registering dynamic tools does not establish that built-in execution, search, plugins, or other local actions are absent. The fetched App Server documentation does not resolve complete exclusion. A read-only sandbox or an approval setting is insufficient evidence. Test the actual provider-visible tool set and execution paths for the selected binary.

Brain currently archives only native files marked append-only and restores them by truncating to saved boundaries. Mutable databases and rewritten native state cannot be added by copying a live file or setting `append_only: true`. Each runner must first demonstrate a coherent, passive checkpoint on a fresh machine. This limitation comes from `Archive.Publish` and `restoreNative`, not from the cloud provider.

Prefer OpenCode as the next full adapter because its documented plugin surface offers more direct control; run the Codex feasibility probe early as well. This ordering is an engineering inference. If OpenCode needs invasive changes and Codex passes the gates, reverse the order.

## MCP and the tool catalog

Use the official **MCP Go SDK** for the second tool transport. Its compatibility table currently lists SDK `v1.7.0+` support for the `2026-07-28` protocol. Select an exact release during implementation. The current HTTP binding has request-scoped JSON/SSE responses and cancels through closing the response stream; older connection/session assumptions should not be copied into a new implementation. [SDK compatibility](https://github.com/modelcontextprotocol/go-sdk), [current transports](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports).

Recommend remote Streamable HTTP tools first, with an explicit configured allowlist and host credentials. Brain's HTTP/SSE tool protocol is still useful and is not MCP. No local MCP subprocess launcher, automatic marketplace installation, unsolicited model sampling, or OAuth broker is required for the first cut. Supporting an external protocol identifier does not create a Brain API version or a compatibility promise.

| Tool family | Initial choice | Why / alternative |
| --- | --- | --- |
| Bucket storage | One external Go S3/R2 service | Reuses the AWS SDK already present. Buckets/prefixes are tool configuration, separate from Brain's checkpoint credentials. |
| Web search | Brave first; Tavily second | Both have direct HTTP APIs. Start with bounded search results and source URLs; add Tavily when its search/extraction workflow is useful. No provider SDK is needed just to make a few JSON requests. |
| Web fetch | External Go HTTP service | Public URL retrieval and extraction have different trust boundaries from Brain's operator-configured tool endpoints. Use explicit network and body bounds; defer browser rendering. |
| Managed execution | Modal Sandboxes first | Command execution, streamed output, time limits, and termination are documented. Python is the primary SDK; Go/JS SDKs are currently beta. Prefer a small separate Python service initially, or accept the beta Go SDK only after feature tests justify it. |
| Machine control | Fly Machines next | Useful when the developer needs explicit machine/image/network control. More lifecycle responsibility than a single execution call. |
| Browser | TypeScript + Playwright service | A managed browser and a user's current browser are separate operating modes. Playwright notes lower fidelity for CDP attachment than its own protocol. Prove tenant/tab scoping before adding user-browser attachment. |
| Alternative sandbox | E2B, later | Its pause/resume facilities are worth comparing if persistent interactive sandboxes become the main workload. Do not implement three compute providers at once. |

Sources: [Brave Search](https://api-dashboard.search.brave.com/app/documentation/web-search/get-started), [Tavily Search](https://docs.tavily.com/documentation/api-reference/endpoint/search), [Modal Sandboxes](https://modal.com/docs/guide/sandboxes), [Modal command streams](https://modal.com/docs/guide/sandbox-spawn), [Modal SDK status](https://modal.com/docs/guide/sdk-javascript-go), [Fly Machines API](https://docs.fly.io/machines/api/machines-resource), [Playwright attachment](https://playwright.dev/docs/api/class-browsertype#browser-type-connect-over-cdp), [E2B persistence](https://docs.e2b.dev/sandbox/persistence).

Tool services may hold a request open or return a job handle with separate status/cancel tools. That distinction stays in their tool schemas and results. Neither implementation requires a Brain job resource or a scheduler that prompts the model to poll.

## Deployment platforms are not interchangeable

| Platform | Fit for Brain | Example to build |
| --- | --- | --- |
| Local process / Docker Compose | Best first developer experience; no paid account needed. | Pi, external tool service, optional PostgreSQL and telemetry collector; deterministic recording-provider mode. |
| Fly Machines | Suitable container host with local working storage. Volumes are local to one machine and are not automatically replicated. | One machine first, R2 or S3 archive, optional PostgreSQL. A later platform-owned router can use `fly-replay` to select an owner. |
| AWS ECS Fargate | Container task storage can hold the disposable cache. | One task first, S3 task-role credentials, PostgreSQL, and explicit replacement/handoff. Replicas need a platform placement mechanism. |
| Cloudflare | Worker tool services can use platform bindings; the Brain executable belongs in a Container. | First a Worker-hosted R2 tool; later a Container + Durable Object routing example with lifecycle/restore tests. |
| Google Cloud Run | Conditional fit for this background-turn workload. | Experimental example only after CPU allocation, routing, shutdown, and memory limits are handled. Default request-based execution is unsuitable after Brain returns `202`. |

Sources: [Fly volumes](https://docs.fly.io/volumes/overview), [Fly routing](https://docs.fly.io/networking/dynamic-request-routing), [Fargate ephemeral storage](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/fargate-task-storage.html), [Cloudflare Containers](https://developers.cloudflare.com/containers/).

Cloud Run's instance-based billing allows CPU outside requests, but it still permits instance termination. Its request timeout can reach 60 minutes, and session affinity is best effort. These facts imply that an ordinary autoscaled service plus sticky cookies does not satisfy Brain's single-owner contract. This is a deployment inference, not a claim that Cloud Run cannot be used. [CPU/background behavior](https://docs.cloud.google.com/run/docs/configuring/billing-settings), [timeouts](https://docs.cloud.google.com/run/docs/configuring/request-timeout), [affinity](https://docs.cloud.google.com/run/docs/configuring/session-affinity?hl=en).

Do not infer turn lifetime from an HTTP response lifetime. Accepted turns, silent tool waits, and checkpoint uploads can all continue after the originating request ends. Runtime hosting must accommodate that behavior.

## Delivery and documentation tooling

- **Keep GitHub Actions.** Publish the verified container to GHCR using `GITHUB_TOKEN`, with provenance attached to the tested digest. Start with native GitHub/Docker actions and a short artifact script; GoReleaser or a separate release orchestrator is not yet necessary. [Publishing images](https://docs.github.com/en/actions/tutorials/publish-packages/publish-docker-images).
- **Use OIDC for AWS qualification.** Scope the CI role to the test bucket and trusted workflow identity. R2 credentials need their own documented mechanism; do not assume AWS's federation applies there. [GitHub OIDC with AWS](https://docs.github.com/en/actions/how-tos/secure-your-work/security-harden-deployments/oidc-in-aws).
- **Use Dependabot before introducing another update service.** It covers Go modules, npm, Docker, GitHub Actions, and later Python dependency manifests. Keep updates reviewable and run the full conformance suite on harness updates. [Dependabot options](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference).
- **Keep the static Fumadocs site.** GitHub Pages is the lowest-account-overhead initial deployment. A repository site needs the `/brain` base path, including search, raw Markdown, and OpenAPI URLs; changing a Next.js base path requires rebuilding. Cloudflare Pages with a root domain remains an alternative if that account/domain is preferred. [Pages workflows](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages), [Next.js base paths](https://nextjs.org/docs/app/api-reference/config/next-config-js/basePath).
- **Reuse Fumadocs components for shell/language tabs.** The installed library already provides this UI; examples should come from tested source files instead of copied snippets. [Fumadocs tabs](https://www.fumadocs.dev/docs/ui/components/tabs).

## Questions that need evidence, not more abstractions

- Can each new harness start with only Brain-bound tools and no startup inference, including after an unclean shutdown?
- What native state is required after compaction, and can checkpoint I/O stay proportional to changed data?
- How much of current sync latency comes from the serial worker, remote round trips, or native file handling?
- Which hosted PostgreSQL connection modes pass the actual adapter without special casing?
- Does an execution provider need durable job tracking, or is a scoped provider handle sufficient for its first workflow?
- Which platform has a demonstrated single-owner routing strategy before advertising multiple replicas?

The linked implementation plans assign concrete probes and acceptance tests to these questions. Live credentials and deployment accounts are prerequisites for later qualification, not for the current planning work or local development.
