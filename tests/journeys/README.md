# Local user journeys

These tests build and run the actual Brain executable, Pi runner, example tool service, and TypeScript/Python clients. Only the model provider and fault-producing external services are fixtures. No model credentials, cloud accounts, Docker, additional packages, or external network services are required.

From the repository root, after the npm installations in the [main README](../../README.md):

```powershell
node scripts/check.mjs
```

That command builds the runner and client, runs every existing check, and runs these journeys. The existing Windows and Linux CI jobs execute the same command; journeys are required and never skipped for missing dependencies. To run just this suite after building the runner and client:

```powershell
node --test tests/journeys/journeys.test.mjs
```

The suite compiles its Go executables into a temporary directory. Each journey gets independent authentication, local state, ports, and a recording model. Cleanup stops owned process trees, including Pi children, before removing temporary files. The crash journeys deliberately kill the server and its runner on both operating systems.

| Journey | Observable guarantees |
| --- | --- |
| Configure a local server | `-check` validates without creating state or calling a model; invalid host settings and external schema references fail. |
| Start and continue a conversation | Create/read/list/send/close, Unicode output, live events, paginated history, accepted input identity, idempotency conflicts, and preserved model context. |
| Authenticate and delegate inspection | Tenant isolation across every session route, tenant-filtered catalogs, reader/participant permissions, own-session inspection, argument/scope separation, and delegation revocation. |
| Use external tools and approvals | Real example echo and inspection tools; approval yes/no, provisional progress, silent waits, independent session progress, and duplicate-decision rejection. |
| Reconnect an observer | SDK cursors and `Last-Event-ID`; replay-to-live continuity; disconnect leaves inference running; lost cursors return `cursor_unavailable`. |
| Queue, interrupt, and close | Bounded admission, `Retry-After`, rejected-key reuse, queued-input cancellation, explicit steering rejection, released execution slots, and cancelled approval cleanup. |
| Handle tool failures | Denial, HTTP failure, malformed JSON, excessive output, connection loss, timeout, redirect, forged control fields, and incomplete SSE become model-visible results without automatic tool replay. |
| Restart after process death | Surviving history and accepted identities remain; torn final records are trimmed; resume is passive; abandoned native/queued work and uncertain tools are not replayed. |
| Recover into a fresh cache | Local miss retrieves external state; uploaded history and native resume boundaries stay distinct; context survives relocation; scoped restore, in-use conflicts, rollback, and lost-tail cursors. |
| Recover from an archive outage | Local accepted input survives, full synchronization queues stop admission, health distinguishes liveness/readiness, and explicit flush restores progress. |
| Diagnose provider and Brain failures | Provider rejection interrupts; malformed runner protocol fails; neither is disguised as a tool error; unrelated conversations continue and logs omit credentials. |
| Repair corrupt local history | Complete/interior corruption fails without silent trimming or fallback; an explicit external restore recovers the saved session. |
| Reject invalid API input | Unsupported harness/provider/tools, unknown fields, invalid delivery, size/page limits, and unconfigured persistence fail before acceptance. |
| Bound observer memory | An event exceeding the live observer budget disconnects that observer; execution and retained history remain usable and can be replayed. |
| Switch SDKs | Python continues a TypeScript conversation, calls tools, streams, deduplicates, flushes/restores, interrupts/resumes, creates sessions, and closes them. |
| Run concurrent tenants | Active execution caps hold, tenant/session/tool scope stays isolated, and native context survives runner eviction without replaying effects. |
| Operate with telemetry | Trace context crosses admission and external tool execution; OTLP exports spans and metrics; exporter failures leave conversations usable. |

The recording provider returns explicit text/tool-call sequences; it does not imitate model reasoning. Assertions inspect its next request to verify context and tool-result delivery. Small bounded waits test absence of automatic model activity; ordering otherwise uses observed HTTP/SSE state and bounded polling, rather than fixed startup sleeps.

This is coverage for the implemented local development profile. It does not qualify live S3/R2/model providers, future harnesses, dynamic configuration, compaction/rewind features not yet exposed, multi-host ownership, production throughput, or large load/soak behavior. Existing Go contract, race, PostgreSQL, Pi, SDK transport, and Docker smoke suites remain required alongside these journeys. See the [verification plan](../../docs/plans/07-verification-and-release.md) for later release gates.
