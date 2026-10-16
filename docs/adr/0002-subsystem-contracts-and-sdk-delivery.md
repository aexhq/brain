# ADR 0002: Subsystem contracts and published SDKs

- **Status:** Accepted
- **Date:** 2026-10-16
- **Extends:** [ADR 0001](0001-local-first-session-server.md)

## Decisions

Session coordination depends on the repository, tool executor, and checkpoint
operations it consumes. The executable remains the composition root: it opens
concrete adapters, supplies dependencies, and closes owned resources after the
engine drains. Interfaces are compile-time extension seams inside this repository;
they are not a dynamically loaded plugin system.

Local metadata and local files are separate responsibilities. The metadata
contract describes create, get, save, ordered tenant listing, host scan, and import.
SQLite implements it; the local store owns directory layout and prepares native
state before metadata creation. The external PostgreSQL index remains eventually
consistent discovery, with a different contract. Sharing SQL infrastructure does
not make these consistency models interchangeable.

The tool registry owns tenant authorization, argument schemas, deadlines,
inspection delegation, and normalized output validation. Transport adapters own
serialization, authentication on the wire, protocol limits, and outcome ambiguity.
HTTP JSON/SSE and MCP implement the same call contract. They never retry actions.
New transports must pass the same policy tests before being wired at startup.

The Go API remains the single source for OpenAPI and SDK wire types. SDK consumers
install published npm/PyPI packages. Package identifiers use semantic versions:
1.0.0 starts the current server's client contract and replaces the predecessor's
incompatible npm API. Breaking client changes require a new major, additions a
minor, and compatible fixes a patch. This authorizes SDK release versions, not
versioned server routes, old runner negotiation, or storage migrations. Releases
record the matching server commit; deployments pin that image and client version.

CI tests installed artifacts on Linux and Windows, including real-server journeys,
and publishes the tested Linux artifacts only after all verification jobs pass.
Registry trust is explicitly configured for the publishing workflow. Missing
registry trust fails publication. Containers and documentation retain their
existing verified deployment path.

## Consequences

Database and tool implementations can change without editing turn scheduling.
Consumers no longer need Go, a checkout, or a TypeScript build to call Brain.
The repo gains two registry artifacts and their release metadata, but keeps one
current server contract and no compatibility layer. Interface changes still need
all implementations, generated docs, and conformance tests updated together.
