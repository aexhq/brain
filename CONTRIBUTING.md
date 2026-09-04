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

## Contracts are the source of truth

The schemas, OpenAPI document, protocol semantics, examples, generators, and conformance fixtures
under [`contracts/`](contracts) define the wire. Generated views are generated, never hand-edited —
CI regenerates them and fails on a diff.

Change a schema and its generated views in the same commit:

```sh
npm run gen
```

`npm run gen` regenerates the contract digests, the provider catalog, and the SDK's TypeScript
types. The Rust types in `crates/brain-protocol` are written by hand and kept in step with the
schemas by the conformance tests, which validate the checked-in examples against both.

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
- Journal every decision before its external effect, and record terminal results before the next
  agent loop activation.
- Keep the `brain` crate free of cloud SDKs. Storage, custody, and runtime behaviour go behind
  public adapters.
- Do not weaken a production invariant to make local development easier.

[`AGENTS.md`](AGENTS.md) has the full working rules for this repository.
