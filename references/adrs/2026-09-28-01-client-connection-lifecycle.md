# Idle client tool connections

Accepted 2026-09-28; implementation and release evidence are tracked separately.

A client defaults to suspending its shared host command stream after 5,000 ms of
confirmed inactivity. `connectionIdleTimeoutMs: 0` keeps it connected for work from
other callers or future autonomous events. Suspension retains the host identity,
callbacks and session handles. Explicit close remains permanent disposal.

Session coordination reports admitted work with scope-owned activity guards. The
host adapter aggregates that activity for its bound sessions. Admission and
conditional suspension share the host lock, so work admitted before cutover keeps
the stream connected. Later work observes a disconnected host. Background tools
retain activity until completion and follow-up work drain; queued Environment
observations acquire activity before being spawned. Pending commands also prevent
suspension. Session status and transport silence cannot establish inactivity.

The command stream starts with a connection-scoped activity snapshot and publishes
updates. A suspend request names the existing connection generation. A stale
request cannot close its replacement. These controls are transient transport state;
they do not add journal records, effect retries or another scheduler.

The SDK retains one registry owner while replacing the stream. Execution-bearing
operations keep it connected and await any pending suspension before reopening.
A suspension response can arrive after the server closes SSE, so reconnect waits
for that response. Read-only history requests do not wake tools. Caller-owned
subscriptions are independent.

This is one pre-launch protocol update across Brain and its consumers. There is no
capability negotiation, legacy transport or deprecated alias. The current Rust
contract generates the HTTP and SDK definitions. Required CI and real process,
background-work, browser and race tests gate publication.
