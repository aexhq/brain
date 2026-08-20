# Working in this repository

- Brain owns the neutral session API, Brain-to-Hand protocol, Brain-side Hand client, and public
  Hand composition ports. Change schemas and generated views together; never hand-edit generated
  contract files.
- `hands` and downstream products consume immutable Brain tags or revisions. Brain must not depend
  on a Hands implementation crate or product-specific runtime.
- Journal every decision before its external effect. Seal the session prefix for its lifetime,
  preserve absent usage counters as absent, record tool intents before dispatch, and record results
  before releasing Hand state. A lost Hand interrupts work and is never replayed.
- Keep the `brain` core independent of cloud SDKs. Put storage, custody, and runtime behaviour behind
  public adapters, and do not weaken production invariants for local development.
- `tests/local_e2e.rs` covers HTTP, SSE, journaling, and real subprocess tools with a scripted model
  provider. Hosted infrastructure and MicroVM gates belong downstream.
- Fail fast, keep comments self-contained, and use plain English.
- Commit style: `area: imperative summary`.
