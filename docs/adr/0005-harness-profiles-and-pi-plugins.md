# ADR 0005: Typed harness profiles and installed Pi plugins

- **Status:** Accepted
- **Date:** 2026-10-22
- **Extends:** [ADR 0003](0003-application-sdk-and-session-bindings.md) and [ADR 0004](0004-service-integration-readiness.md).

## Decision

The Node application installs named `Pi(...)`, `Codex(...)`, and `OpenCode(...)`
profiles in `BrainServer`. The server owns runtime packages, process settings,
plugin configuration, endpoints, and credentials. A client selects an installed
profile through a typed constructor; the HTTP contract retains its harness name.
Remote clients cannot install packages or change a profile's server settings.

The npm platform packages contain the Go executable and the complete locked
runtime dependency tree selected for that OS and CPU. The portable package
contains the client and application entry point. Installation runs no scripts.

Pi profiles can install the pinned web-access, subagents, and MCP packages.
These are trusted server extensions. Clients and models cannot supply extension
paths or code. Selecting a profile enables its configured plugin tool subset in
addition to the session's ordinary tool bindings. Plugin tools are not ambient
user or project resources.

The runner reports native tool descriptors when attaching. A native tool callback
passes through Brain's existing call recording and cancellation scope before
Brain dispatches execution to the same attachment. Progress and results use the
ordinary event contract. An explicit plugin selection does not grant another
plugin's tools or change the application's bound tools.

The packages use process-global configuration, so plugin profiles have one
session and one active turn per host. Their private configuration lives in the
owned temporary host directory. Session state remains in native checkpoints.

Subagents run in the foreground using the parent's model, credentials, request
observer, and bound application tools. Child transcripts join the checkpoint.
Capability ceilings exclude child extensions and uninstalled agents. Background
runs, scheduled inference, remote placement, and model-supplied workflow code
are excluded from this profile. Model parameters cannot request host commands,
filesystem placement, sharing, or management actions. Web access uses raw results without automatic
browser or answer-model execution. MCP uses the package's host-managed profile
with a frozen catalog and no automatic action replay.
