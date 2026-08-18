# Working in this repository

* Wire formats are owned by `aexhq/aex` (`aex-contracts`, session API v1 + ABI v1) and the hand
  crates by `aexhq/hands` — both consumed by git tag. Never re-describe a wire format here;
  change it there first, then bump the tag.
* Design authority is `aex-research/docs/ARCHITECTURE-v1.md` (private repo). The invariants that
  bind the brain: one durable DynamoDB write per decision (D9); the prefix is sealed for the
  session's life (D11); absent ≠ zero on usage (D10); tool intents journal before dispatch and
  results journal before `release`; a lost hand is `hand_lost`, interrupted, never replayed (I10);
  the brain mints every presigned URL (I8); never attach an execution role to a hand MicroVM.
* The brain is host-side Rust and builds/tests on any OS. Three crates: `brain` (the
  substrate-generic core — NO cloud SDK may ever appear in its dependency tree), `brain-aws`
  (the AWS adapters), `brain-server` (the composed binaries; local by default, AWS by
  `AEX_MODE=aws`). Substrate behaviour goes behind `brain::adapter`; never let a local-mode
  convenience weaken a production invariant, and never re-fuse the core to a cloud.
* `tests/local_e2e.rs` drives the whole loop (HTTP + SSE + journal + real subprocess tools)
  with a scripted provider and runs in CI. `bin/m0` is the AWS gate: real AWS (eu-west-1 dev
  plane) + real provider keys.
* Fail fast; plain English in comments. Commit style: `area: imperative summary`.
