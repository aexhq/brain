# Working in this repository

- Brain owns the neutral session, remote Environment, and Agentloop contracts.
  Change schemas and generated views together; never hand-edit generated contract files (CI
  regenerates and diffs them).
- `hands` and downstream products consume immutable Brain tags or revisions. Brain must not depend
  on a Hands implementation crate or product-specific runtime.
- Journal every decision before its external effect. Seal the session presentation and bindings for
  its lifetime, preserve absent usage counters as absent, and record terminal results before the
  next Agentloop activation. Redelivery must preserve the operation ID, request digest, and logical
  binding; never replay an ambiguous effect to a replacement physical target.
- Keep the `brain` core independent of cloud SDKs. Put storage, custody, and runtime behaviour behind
  public adapters, and do not weaken production invariants for local development.
- Real Linux Loophost, image, HTTP, SQLite recovery, remote-model, and remote-Environment gates run
  in CI. Hosted directory, placement, and cloud infrastructure gates belong downstream.
- Fail fast, keep comments self-contained, and use plain English.
- Commit style: `area: imperative summary`.
