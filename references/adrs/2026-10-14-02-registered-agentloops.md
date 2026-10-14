# ADR-056: Register agentloop workers and select them per session

- Decision date: 2026-10-14
- Status: Accepted
- Supersedes the single-implementation selection in [ADR-053](2026-10-14-01-startup-harness.md).

## Context

One Brain server needs to serve Pi, Codex, OpenCode and custom implementations
concurrently. Clients need behavior settings, but installing executable code remains
an operator responsibility. Re-exporting adapters from Brain makes extensions wrappers
around implementation code that belongs outside the neutral server.

## Decision

At startup, register a map from names to installed commands, default configuration,
worker counts and per-worker activation limits. Session creation supplies a registered
name and behavior configuration. Unknown names and executable fields fail creation.
Registrations cannot be changed through the session API.

Brain consumes extensions through its public stdio protocol. Extension packages own
upstream dependencies, adapters, native state handling, binaries and conformance tests.
Their client helpers produce serializable selections and do not start runtimes.

Each open session stays with one process. Worker pools admit concurrent activations;
one session still runs one activation at a time. Closing an idle session releases its
worker binding and resources, while native files remain available for reopening.
Worker failure never triggers an automatic restart or effect replay. Healthy workers
continue serving other sessions. Existing durable callback barriers remain in force.

## Alternatives and consequences

A single process for all implementations cannot isolate their failures or allocate
capacity independently. Session-supplied executable code would cross the operator
trust boundary, so it remains unsupported.

The base Brain image is independent of upstream agent packages. An extension image
composes a pinned Brain image with the installed upstream runtimes. Operators can also
build their own composition. Native state is stored under `agentloops/{name}`; this
prelaunch release uses a new `brain-data/3` directory rather than migrating old data.

## Sources

- User decision to register implementations at startup and select/configure them per session.
- [Implementation and release plan](../changes/2026-10-14-registered-agentloops.md).
