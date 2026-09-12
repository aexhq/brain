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

CI additionally runs real loop worker and HTTP lifecycle integration tests, the locked Python
Environment preparation test, and an image smoke test. Run the Python check with
`BRAIN_TEST_UV=/path/to/uv node --test examples/python-environment.test.mjs`; CI installs uv 0.8.15. Performance probes are optional diagnostics during pre-launch iteration. See [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

[SDK user journeys](tests/journeys/README.md) run against real Linux servers and workers with four
isolated suites in parallel. They cover the public SDK lifecycle, tools, placement, events, and
recovery on pull requests affecting Rust, the SDK, examples, or integration fixtures. Run them with
`npm run test:journeys` after the documented builds. PR CI selects affected checks from the full PR
diff; ordinary prose changes skip builds, while the generated configuration reference still runs
contract verification. Pushes to `main` run the full suite.

Before promotion, run `media-integration.yml` on the exact candidate commit. Its protected
`media-integration` environment needs `OPENAI_API_KEY` and `VERCEL_AI_GATEWAY_API_KEY`.
OpenAI and Anthropic models use Vercel's Responses endpoint and gateway key, alongside a direct
OpenAI reference check. Anthropic Messages payloads remain covered by the regular adapter tests.
The probe reads the committed image fixture and PDF report text to verify user media,
Tool-result files, continuation and Responses compaction. Missing credentials fail the check;
promotion requires a successful run for the same commit. For a local probe, run
`cargo run --locked -p brain --example media-probe -- --help` for the required environment variables
and supply accessible HTTPS URLs for the two fixtures. Keep secrets out of shell arguments and logs.

Main CI publishes immutable candidate images. Only release promotion moves the image and npm
`latest` tags, after the media gate passes.

## The Rust types are the source of the contracts

The wire is defined once, as the types in [`crates/brain-protocol`](crates/brain-protocol) and the
`#[utoipa::path]` annotations on the handlers in [`crates/brain-http`](crates/brain-http). Each
crate renders its own `generated/contract/` directory: brain-protocol the JSON Schemas by
`schemars` and the code catalogue from `brain_protocol::codes`, brain-http the OpenAPI document by
`utoipa`, brain the provider list from the vendored `catalog/` snapshot. The WIT the loop host
implements is written by hand under [`crates/brain-env/wit`](crates/brain-env/wit). The
SDK's `src/generated` is rendered from the crates' contracts in turn.

Change a type and rerun the renderers in the same commit:

```sh
npm run gen
```

`npm run gen` runs `cargo run -p <crate> --bin <crate>-contract` for each crate and rebuilds the SDK's
TypeScript types. CI runs the same command and fails on a diff, so a rendered file
cannot be edited by hand and a type cannot change without its contract following. The conformance
tests validate the checked-in examples against the rendered schemas.

## Documentation

Pages live in [`docs/`](docs) and are rendered at
[aex.dev/brain/docs](https://aex.dev/brain/docs). Change behaviour and its page in the same pull
request.

Architecture rationale lives in [`references/adrs/`](references/adrs/README.md). Record new
decisions as `YYYY-MM-DD-NN-topic.md` with context, status, alternatives, consequences, and
sources. Link any superseded decision instead of rewriting its history.

The API reference is not written by hand. It is generated from
[`crates/brain-http/generated/contract/session/v1/openapi.yaml`](crates/brain-http/generated/contract/session/v1/openapi.yaml)
at site build time, so it
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
