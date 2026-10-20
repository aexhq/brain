# 11: Application SDK and session-bound execution

Implements [ADR 0003](../adr/0003-application-sdk-and-session-bindings.md), including the decisions recorded in Copilot session `0b8f387c-67a2-48d2-96e6-a5390258fad6`. The starting runtime is `415b7c1`; the subsequent five local commits contain design and qualification evidence only.

[ADR 0004](../adr/0004-service-integration-readiness.md) and [plan 12](12-service-integration-readiness.md) extend the implemented baseline with service-integration requirements. They assign inspection tools to platforms and add browser, model-credential, usage and common host-pool work; the original slice's exclusions below do not defer those new requirements.

## Decisions

- Keep the Go coordinator. Publish a Go composition package and make the stock executable a consumer of it. Application code is trusted; session requests cannot install code.
- Retain named managed tools and add provider bindings with explicit aliases, operation selection, parameters, and optional private credentials. Persist resolved descriptors so a deployment cannot silently change a saved catalog.
- Use one policy path for managed functions, direct HTTP/SSE, MCP, and attached runtime callbacks. Successful output schemas describe `structured_content`; errors retain their own envelope.
- Implement reverse SSE commands plus HTTP responses, and JSON messages over WebSocket for bidirectional RPC. Both use the same bounded registration/call state. There is no transport fallback or action replay. Runtime handlers remain in the client.
- Encrypt direct credentials with Go's AES-GCM implementation before persistence. An application key provider supplies a stable active key and retained decryption keys. Authenticate tenant, session, and binding identity. Store private records separately from public session JSON, and include encrypted records in authorized checkpoints. Existing records, including revocations, win over older restored records.
- Make host occupancy and active-turn limits explicit. Shared hosts reserve capacity before opening a native session and release it after detachment. Pi is the first production adapter; Codex and OpenCode must meet the same catalog, recovery, isolation, and process-lifetime gates before publication as supported integrations.
- Keep browser authentication and cross-machine placement outside this change. Backend SDK callback connections use scoped authentication; installed HTTP references cover serverless tools.

## Implementation sequence

| Slice | Changes | Required evidence |
| --- | --- | --- |
| Application composition | Public contracts, trusted handlers, shared startup validation, owned-resource shutdown, stock CLI using the SDK | Build a separate Go module without `internal` imports; exercise startup failure and shutdown |
| Session bindings | Provider authorization, explicit aliases/operations, frozen descriptors, structured output, managed HTTP/MCP building blocks | Two tenants and two bindings of one provider; collision/revocation/catalog-change rejection; no partial admission |
| Credential custody | Private ingress, authenticated encryption, atomic SQL admission, checkpoint export/import, rotation/revocation | Restart and cold restore; no plaintext in SQL/WAL/archive/public data; tampering/key loss fail; older restore preserves revocation |
| Runtime callbacks | Registration, reverse SSE/HTTP and WebSocket RPC, progress/cancel/result correlation, SDK lifecycle | Unavailable before dispatch; unknown after dispatch; no replay; stale/duplicate/foreign replies rejected; restored descriptors rebind |
| Harness hosts | Bounded shared hosts, attachment identity, per-session routing/state, process-tree ownership | Concurrent admission limits; cancellation/eviction/crash with peers; empty/exact catalogs; passive restore and changed-data checkpoints |
| Integration packages | MCP provider lifecycle, optional scoped session reads, qualified Codex/OpenCode adapters and provider helpers | Explicit discovery and selection; process loss; unauthorized targets; adapter conformance against pinned upstreams |
| Delivery | Generated contract and clients, runnable examples, guides/reference, roadmap evidence, package versions | Full local checks; Linux/Windows CI, race, PostgreSQL, containers, CodeQL; protected merge; exact-artifact publication and docs smoke |

## Failure boundaries

Admission validation prevents persisted sessions with unusable or unauthorized bindings. Descriptor comparison prevents reinterpretation after redeployment. Credential authenticated data prevents ciphertext substitution between tenants/bindings. Execution generation and call identity prevent stale connections completing a newer call. Capacity reservations prevent concurrent opens exceeding configured occupancy. Each failure rejects admission, stops the affected execution, or produces a classified tool result; none retries an action or silently changes credentials/tools.

## Verification and delivery

Run `node scripts/check.mjs` without exclusions. CI additionally owns PostgreSQL, race detection, Linux container smoke, and CodeQL. A missing integration dependency is a failure, not a pass. Record adapter qualification separately from live cloud-provider qualification. Merge only after the protected gate succeeds. Main CI publishes the tested images, deploys GitHub Pages, and dispatches SDK publication; verify every resulting workflow and public artifact before reporting release completion.

## Implementation evidence

The public package and stock CLI share composition and startup validation. `TestSeparateGoApplication` builds a separate Go module against public imports and exercises passive session creation, tenant-scoped reads, and shutdown. Managed binding tests cover aliases, resource parameters, schema drift, collisions, authorization, and structured results. Credential tests inspect SQL/WAL and archives, restart and cold restore, missing records/keys, substitution, rotation, and revocation surviving older imports.

The installed TypeScript package exercises both SSE/HTTP and WebSocket callbacks; the installed Python package exercises SSE/HTTP. Journeys cover progress, trusted scope, disconnection, server restart, rebind, observer separation, and session closure. MCP tests use official HTTP and stdio client/server transports, including an owned process that exits during dispatch. The sessions provider tests permitted and forbidden targets and excludes private records.

Pi's shared-host test opens eight native sessions with different catalogs and context, interrupts one while a peer continues, and relocates one checkpoint while peers remain. Go host tests cover concurrent admission, active-turn limits, detachment capacity reuse, shared crashes, passive reopen, and abrupt owner death with a real descendant. The full check and CI gate own platform results.

## Remaining adapter qualification

Codex 0.163.0 and OpenCode SDK/plugin 2.0.30 are not shipped as supported adapters in this slice. Re-inspecting the pinned packages and App Server documentation confirms that catalog exclusion alone is insufficient. Codex still needs per-thread detach/flush, compaction recovery, and cold relocation evidence; unsubscribe has a grace period and is not proof of released residency. OpenCode's public export returns a complete session and message list, while its default startup resumes suspended sessions. A passive embedding and incremental per-session persistence adapter need their own conformance evidence. Do not copy a whole shared database each turn or advertise the original spikes as that evidence. [Harness qualification](../research/harness-probes.md) and [plan 03](03-harness-adapters.md) retain this work.

This implements the selected application and Pi runtime surface; it does not claim completion of every future integration mentioned in ADR 0003.
