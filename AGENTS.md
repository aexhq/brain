# Working in this repository

- Brain owns the neutral session API, Brain-to-Hand protocol, and public Hand composition ports.
  Change schemas and generated views together; never hand-edit generated contract files (CI
  regenerates and diffs them).
- `hands` and downstream products consume immutable Brain tags or revisions. Brain must not depend
  on a Hands implementation crate or product-specific runtime.
- Journal every decision before its external effect. Seal the session prefix for its lifetime,
  preserve absent usage counters as absent, record tool intents before dispatch, and record results
  before releasing Hand state. Redelivery of the same operation ID and digest to the same
  recoverable binding is required; never replay an ambiguously lost operation to a replacement
  physical target or customer process.
- Keep the `brain` core independent of cloud SDKs. Put storage, custody, and runtime behaviour behind
  public adapters, and do not weaken production invariants for local development.
- `crates/brain-server/tests/standalone_e2e.rs` covers real HTTP, finite SSE, durable SQLite
  journal/storage reopen, and real Node managed-tool subprocesses with a scripted model provider.
  Hosted infrastructure and MicroVM gates belong downstream.
- `tools/e2e-smoke.mjs` is the release gate for the published pair: through the installed
  `@aexhq/brain` archive and the built server it drives a hosted engine capability and the full
  four-component composition, a Tool running its sealed bundle in a bound component Environment,
  then ends past a release the Environment refuses. The tagged publish workflow runs it against
  the exact staged versions between `stage` and `promote`, and promotion fails closed without
  that evidence.
- Fail fast, keep comments self-contained, and use plain English.
- Commit style: `area: imperative summary`.
