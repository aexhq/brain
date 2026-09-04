# Contributing

Brain is under early development. Contracts are replaced in place until the first stable release,
so a change that would be a breaking change later is usually just a change today.

## Setup

You need Rust 1.97 and Node 22 or newer.

```sh
cargo build --workspace
npm ci
```

## Verification

Everything CI runs, in the order it will fail:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm ci
npm test
npm run package-smoke
```

CI additionally runs the loop worker integration tests, an image smoke test, and a resident-memory
bound against a live server. See [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

## The Rust types are the source of the contracts

The wire is defined once, as the types in [`crates/brain-protocol`](crates/brain-protocol) and the
`#[utoipa::path]` annotations on the handlers in [`crates/brain-http`](crates/brain-http).
[`contracts/`](contracts) is rendered from them: the JSON Schemas by `schemars`, the OpenAPI
document by `utoipa`, the code catalogue from `brain_protocol::codes`. Only `agentloop.wit` and
the `examples/` directories are written by hand. The SDK's `src/generated` is rendered from
`contracts/` in turn.

Change a type and rerun the renderers in the same commit:

```sh
npm run gen
```

`npm run gen` runs `cargo run -p brain-contracts`, regenerates the provider catalog, and rebuilds
the SDK's TypeScript types. CI runs the same command and fails on a diff, so a rendered file
cannot be edited by hand and a type cannot change without its contract following. The conformance
tests validate the checked-in examples against the rendered schemas.

## Documentation

Pages live in [`docs/`](docs) and are rendered at
[aex.dev/brain/docs](https://aex.dev/brain/docs). Change behaviour and its page in the same pull
request.

The API reference is not written by hand. It is generated from
[`contracts/session/v1/openapi.yaml`](contracts/session/v1/openapi.yaml) at site build time, so it
cannot drift from the contract.

Code in the documentation comes from real files in [`examples/`](examples), which `npm test` checks.
Do not paste a snippet into a page — reference the example.

## Conventions

- Commit messages are `area: imperative summary`.
- Fail fast. Keep comments self-contained. Write plain English.
- Journal every effect before it happens, and record terminal results before the loop sees them.
- Keep the `brain` crate free of cloud SDKs. Storage, custody, and runtime behaviour go behind
  public adapters.
- Do not weaken a production invariant to make local development easier.

[`AGENTS.md`](AGENTS.md) has the full working rules for this repository.
