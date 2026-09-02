# Working in this repository

- Brain owns the neutral session, remote Environment, and Agentloop contracts.
  Change schemas and generated views together; never hand-edit generated contract files (CI
  regenerates and diffs them).
- `hands` and downstream products consume immutable Brain tags or revisions. Brain must not depend
  on a Hands implementation crate or product-specific runtime.
- Journal every decision. The journal is written behind the turn and is not a durability fence:
  Brain does not fsync, and a crash may lose its tail. Seal the session's tool catalogue and
  bindings for its lifetime, preserve absent usage counters as absent, and record `*_ended` or
  `*_failed` results before the next Agentloop activation. A record is named by session id and sequence and
  nothing else; redelivery must preserve that pair and the logical binding, and never replay an
  ambiguous effect to a replacement physical target. `DECISIONS.md` records why.
- Keep the `brain` core independent of cloud SDKs. Put storage, custody, and runtime behaviour behind
  public adapters, and do not weaken production invariants for local development.
- Real Linux Loophost, image, HTTP, journal recovery, remote-model, and remote-Environment gates run
  in CI. Hosted directory, placement, and cloud infrastructure gates belong downstream.
- Fail fast, keep comments self-contained, and use plain English.
- Documentation lives in `docs/` and ships to aex.dev/brain/docs. Change behaviour and its page in
  the same pull request. The API reference is generated from `contracts/session/v1/openapi.yaml`;
  never write it by hand. Setup and verification commands live in `CONTRIBUTING.md`.
- Commit style: `area: imperative summary`.
