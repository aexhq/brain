# Working in this repository

- The Rust types in `crates/brain-protocol` and the `#[utoipa::path]` annotations in
  `crates/brain-http` are the only source of the session, Environment, and Agentloop contracts.
  `contracts/` is rendered from them by `cargo run -p brain-contracts` (only `agentloop.wit` and
  `examples/` are written by hand), and the SDK's `src/generated` from `contracts/` by
  `npm run gen`. Never edit a rendered file; CI regenerates and diffs them. To change a
  contract, change the type or the annotation and run `npm run gen`.
- `hands` and downstream products consume immutable Brain tags or revisions. Brain must not depend
  on a Hands implementation crate or product-specific runtime.
- Journal every effect before it happens. The local store must durably commit the intent before
  dispatch. A session's Tool catalogue and bindings do not change after create; preserve absent
  usage counters as absent, and record `*_ended` or `*_failed` results before the loop sees the
  result. A record is named by session id and sequence and nothing else. Brain sends an effect once,
  never retries it automatically, and records an unknown outcome when a remote result is uncertain.
  `DECISIONS.md` records why.
- Keep the `brain` core independent of cloud SDKs. Put storage, custody, and runtime behaviour behind
  public adapters, and do not weaken production invariants for local development.
- Real Linux Loophost, image, HTTP, journal recovery, remote-model, and remote-Environment gates run
  in CI. Hosted directory, placement, and cloud infrastructure gates belong downstream.
- Fail fast, keep comments self-contained, and use plain English.
- Documentation lives in `docs/` and ships to aex.dev/brain/docs. Change behaviour and its page in
  the same pull request. The API reference is generated from `contracts/session/v1/openapi.yaml`;
  never write it by hand. Setup and verification commands live in `CONTRIBUTING.md`.
- Commit style: `area: imperative summary`.
