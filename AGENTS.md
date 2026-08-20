# Working in this repository

* Neutral session and Brain↔Hand wire formats are owned here. `hands` and downstream products such
  as Aex consume immutable Brain tags or revisions. Change schemas and generated views together;
  never hand-edit generated contract files.
* Brain owns the Brain-side Hand client and the public Hand composition ports. It must not depend on
  a Hands implementation crate. `hands` depends on Brain and implements the protocol/ports; Aex or
  another downstream server composition wires a selected adapter into Brain.
* Design authority is `aex-research/docs/ARCHITECTURE-v1.md` plus its accepted
  `brain-independent-product-design.md` amendment (private sibling repo). The invariants that
  bind the brain: one durable DynamoDB write per decision (D9); the prefix is sealed for the
  session's life (D11); absent ≠ zero on usage (D10); tool intents journal before dispatch and
  results journal before `release`; a lost hand is `hand_lost`, interrupted, never replayed (I10);
  the brain mints every presigned URL (I8); never attach an execution role to a hand MicroVM.
* The brain is host-side Rust and builds/tests on any OS. `brain` is the substrate-generic core — NO
  cloud SDK or Hands implementation may appear in its dependency tree. Cloud/storage behavior goes
  behind public adapters; never let a local-mode convenience weaken a production invariant, and
  never re-fuse the core to a cloud.
* `tests/local_e2e.rs` drives the whole loop (HTTP + SSE + journal + real subprocess tools)
  with a scripted provider and runs in CI. `bin/m0` is the AWS gate: real AWS (eu-west-1 dev
  plane) + real provider keys.
* Fail fast; plain English in comments. Commit style: `area: imperative summary`.
