# Brain

A standalone agent session server with distributed tools and local working state.

Brain owns session transport, tenant isolation, persistence, and bounded execution. Harnesses own model calls and context. Tools run in external services, including approvals, storage, browsers, and code execution.

Implementation is in progress. See the [roadmap](docs/roadmap.md), [architecture](docs/adr/0001-local-first-session-server.md), and [implementation plans](docs/implementation-plan.md). Supported behavior is established by executable tests; planned integrations are not release claims.

