# Working in this repository

- The Rust types in `crates/brain-protocol`, the `#[utoipa::path]` annotations in
  `crates/brain-http`, and the vendored snapshot under `catalog/` are the only source of the
  session, Environment, Agentloop, and provider contracts. Each crate renders its own
  `generated/contract/` with `cargo run -p <crate> --bin contract`, and the SDK's
  `src/generated` is rendered from those by `npm run gen`. The WIT under
  `crates/brain-loophost/wit/` is written by hand. Never edit a file under `generated/`; CI
  regenerates and diffs them. To change a contract, change the type or the annotation and run
  `npm run gen`.
- `hands` and downstream products consume immutable Brain tags or revisions. Brain must not depend
  on a Hands implementation crate or product-specific runtime.
- Journal every effect before it happens. The local store must durably commit the intent before
  dispatch. A session's Tool catalogue and placements do not change after create; preserve absent
  usage counters as absent, and record `*_ended` or `*_failed` results before the loop sees the
  result. A record is named by session id and sequence and nothing else. Brain sends an effect once,
  never retries it automatically, and records an unknown outcome when a remote result is uncertain.
  The ADRs in [references/adrs](references/adrs/README.md) record why.
- Keep the `brain` core independent of cloud SDKs. Put storage, custody, and runtime behaviour behind
  public adapters, and do not weaken production invariants for local development.
- Real Linux Loophost, image, HTTP, journal recovery, remote-model, and remote-Environment gates run
  in CI. Hosted directory, placement, and cloud infrastructure gates belong downstream.
- Fail fast, keep comments self-contained, and use plain English.
- Documentation lives in `docs/` and ships to aex.dev/brain/docs. Change behaviour and its page in
  the same pull request. The API reference is generated from
  `crates/brain-http/generated/contract/session/v1/openapi.yaml`;
  never write it by hand. Setup and verification commands live in `CONTRIBUTING.md`.
- Commit style: `area: imperative summary`.
