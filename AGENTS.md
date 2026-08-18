# Working in this repository

* Wire formats are owned by `aexhq/aex` (`aex-contracts`, session API v1 + ABI v1) and the hand
  crates by `aexhq/hands` — both consumed by git tag. Never re-describe a wire format here;
  change it there first, then bump the tag.
* Design authority is `aex-research/docs/ARCHITECTURE-v1.md` (private repo). The invariants that
  bind the brain: one durable DynamoDB write per decision (D9); the prefix is sealed for the
  session's life (D11); absent ≠ zero on usage (D10); tool intents journal before dispatch and
  results journal before `release`; a lost hand is `hand_lost`, interrupted, never replayed (I10);
  the brain mints every presigned URL (I8); never attach an execution role to a hand MicroVM.
* The brain is host-side Rust and builds/tests on any OS. `bin/m0` needs real AWS
  (eu-west-1 dev plane) and real provider keys.
* Fail fast; plain English in comments. Commit style: `area: imperative summary`.
