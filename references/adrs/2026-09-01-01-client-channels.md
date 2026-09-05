# ADR-016: Terminate application Tool channels in Brain

- Decision date: 2026-09-01
- Status: Superseded
- Compiled: 2026-09-05

Superseded by: [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md).

## Context

Application tools ran behind an extra env-app callback router. Browser and local-machine handlers needed inbound connectivity or a separate deployment even though their code already lived in the application.

## Decision

Move routing into Brain and use outbound SSE plus HTTP results. The September 1 implementation introduced a session-scoped share key and serve feed, removed callback WebSocket/signed-POST transports, and placed tools with `{ env, ...options }` instead of useIn.

## Alternatives considered

A mandatory app backend or separate env-app service contradicted direct SDK use. WebRTC/WebTransport and push callbacks added connection machinery for a server that was already reachable. The user also rejected a generic third-party HTTP-tool hosting service: credentials and calls belong inside the tool implementation.

## Consequences

The direction of outbound application hosting remains, but the session serve/share-key interface was replaced by the resident host protocol in [ADR-033: Distinguish resident Tools from explicitly placed extensions](2026-09-05-02-placement.md). Current disconnect behavior reports uncertainty without replay; do not revive old reconnect-and-serve-pending semantics from this record.

## Sources

- Brain implementation/history: [8ad9f2f](https://github.com/aexhq/brain/commit/8ad9f2f50e8d52b74f76ba8860008e3cec80a1b2), [9662144](https://github.com/aexhq/brain/commit/96621447223b2eee107b18d8814d2f91552faf88), [d473e76](https://github.com/aexhq/brain/commit/d473e76842c0e19f74067cefd5aefcab0596ac70), [a21ae6e](https://github.com/aexhq/brain/commit/a21ae6e5c46ea2bffd9fe96aebb45471f6f1ac45).
- Current reference: [docs/guides/app-tools.mdx](../../docs/guides/app-tools.mdx).
- Current reference: [docs/concepts/tool.mdx](../../docs/concepts/tool.mdx).
- [Claude session `d052a0cb-6a05-467f-89a3-fc10945062d9`](SOURCES.md#session-d052a0cb-6a05-467f-89a3-fc10945062d9), 2026-09-01T11:10:38.522Z: User asks for tool dispatch to browsers, servers, and local computers; subsequent corrections remove backend and third-party-call assumptions.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
