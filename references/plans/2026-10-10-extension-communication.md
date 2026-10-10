# Encapsulate extension communication

Brain owns reusable communication adapters so extension authors implement their runtime and
resource lifecycle without copying Brain's wire schemas, callback requests or HTTP server glue.
The existing EnvironmentAdapter and ExecutionServices remain the forward and reverse boundaries.
The environment/v1, host, journal and WIT contracts retain their current meaning.

## Implementation

1. Extract SDK HTTP requests, SSE decoding and I/O lifetime into one internal transport module.
   BrainClient composes that module with the existing HostPump. Closing a client must still
   abort and settle its own requests and streams without ending retained sessions.
2. Provide a public environment protocol adapter and scoped callback client using Brain's
   generated schemas. Reuse installed Zod for runtime validation. Invalid wire messages fail
   before provider effects; malformed completion acknowledgements leave an unknown outcome.
3. Provide an explicitly imported Node HTTP server binding with authentication, bounded request
   decoding and shutdown. Keep provider-specific callback relays outside the generic protocol.
   Route typed environment contexts and remote Agentloop/Tool examples through shared callbacks.
4. Migrate official environment helpers and process bridges to the public SDK. Delete their
   duplicate protocol schema, callback POST and HTTP serving implementations. Tool-only
   environments retain their finish-grant requirement; Agentloops do not inherit it.
5. Move Environment HTTP client policy into its adapter. Disable automatic request retries and
   redirects explicitly. Keep connection pooling, response limits and receipt correlation.
6. Update the authoring guide and protocol reference with the public adapters and ownership
   rules. Publish a compatible SDK minor release and matching extension updates; consume exact
   versions and immutable Brain revisions downstream.

## Semantics and verification

- Journal intent before sending and commit results before acknowledgement or loop exposure.
  Reconnect can reopen a channel but never replays an effect. No new delivery queue is introduced.
- A Tool return, emitted result and explicit finish remain different operations. Preserve
  background callback lifetime, emission order, cancellation and original deadlines.
- Operation context, execution services and binding reporter keep distinct credentials and
  lifetimes. A retained reporter cannot gain execution or controller authority.
- Preserve provider failures, transport failures and unknown outcomes separately, including
  structured error details. Credentials never enter journal records or ordinary logs.
- Preserve host connection generations, bounded queues, overload behavior and idle suspension.
  Keep model transport and session-history replay separate from host command delivery.
- Exercise valid and invalid generated envelopes, Tool and Agentloop callbacks, retained
  reporters, authentication and body limits, lost acknowledgements without retry, split SSE
  frames, and client disposal. Reuse existing lifecycle/race/recovery tests.
- Run all checks in CONTRIBUTING.md and required CI, including real Linux worker, HTTP,
  recovery, remote Environment, model and image gates. Run official extension/provider gates
  and website checks before their respective releases.

## Release and deployment

Merge only after required CI succeeds. Tag exact merge commits with immutable annotated
release/sha-* tags. Stage Brain's existing candidate image and SDK archives, run the protected
media gate, and promote those same artifacts. Pin the staged SDK and Brain revision in
extensions, pass their CI and Modal gate, and stage/promote their exact archives. Update the
website's documentation revision and deploy through its CI-gated Vercel workflow.

Production runs in the existing Railway project recorded in Platform's 2026-10-08 migration.
Pin the admitted Brain and Aex image digests in Platform, pass its complete CI including the
combined runtime and paired recovery gate, and deploy the exact main-run archive to both existing
Railway services. Take a completed paired backup before rollout and verify retained sessions,
the separate HTTP controller, real model calls, host event streaming and attachment storage.
Promote the same staged package archives after production proof, and record the deployed tuple
and evidence in Platform's release log. AWS infrastructure remains retired.

## References

- [Environment protocol](../../docs/reference/environment-protocol.mdx)
- [Send each effect once](../adrs/2026-09-05-04-send-once.md)
- [Environment control and capability lifetimes](../adrs/2026-09-25-01-environment-events-and-control.md)
- [Host connection lifecycle](../adrs/2026-09-28-01-client-connection-lifecycle.md)
- [Contributor checks](../../CONTRIBUTING.md)
