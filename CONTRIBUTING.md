# Contributing

Brain is under early development. Contracts are replaced in place until the first stable release,
so a change that would be a breaking change later is usually just a change today.

## Setup

You need Rust 1.97.1, Node 22.19 or newer and Python 3.13.
On Windows with a Python launcher shim, set `BRAIN_TEST_PYTHON` to the absolute interpreter path.

```sh
cargo build --workspace
npm ci
```

## Verification

Run the local checks before opening a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
npm ci
npm audit --audit-level=high
npm run gen
npm test
npm run package-smoke
```

### Session API v2 source distribution

The new Go service and separate supplied-code worker are source-only until their exact-commit
release gates are admitted. They do not replace a retained production data directory.
Use Go **1.27.2** (pinned in `go.mod`), Node 24.17 or newer, and Python 3.12 or newer for the
new SDK and worker journeys. Keep the existing Rust/runtime gates above.

```sh
npm run gen
go test ./internal/... ./adapters/... ./cmd/...
go vet ./internal/... ./adapters/... ./cmd/...
go build -o .build/brain ./cmd/brain
go build -o .build/brain-worker ./cmd/brain-worker
npm run build -w packages/brain-sdk
node --test --test-concurrency=1 tests/greenfield/journeys.test.mjs
node --test --test-concurrency=1 tests/greenfield/managed.journeys.test.mjs
```

On Windows, build `.build\brain.exe` and `.build\brain-worker.exe`. Put Go/Rust scratch and
build directories inside the checkout; the journey scripts put their state in `.test-output`
and remove only resources they create. Managed journeys require a responsive Linux Docker
engine and these immutable, pre-pulled base images:

```sh
docker pull node@sha256:862263c612aa437e3037674b85419622a9d93bff80aa1eee5398dfe686375532
docker pull python@sha256:78387bc3881b8273120a12ebe6c1ab22b018ccc2c9adf565ae1ac9b536e184ea
```

For a manual trusted-development worker, configure:

| Process | Variable | Value |
| --- | --- | --- |
| Worker | `BRAIN_WORKER_TRUSTED_DEVELOPMENT` | `1`; explicitly trusts supplied development code |
| Worker | `BRAIN_WORKER_KEY` | A retained random signing key of at least 32 bytes |
| Worker | `BRAIN_WORKER_DATA` | A new, private receipt directory |
| Worker | `BRAIN_WORKER_CALLBACK_ORIGINS` | Brain's exact origin, such as `http://127.0.0.1:8080` |
| Worker | `BRAIN_WORKER_NODE_IMAGE`, `BRAIN_WORKER_PYTHON_IMAGE` | The immutable references above |
| Brain | `BRAIN_DEVELOPMENT` | `1`; local compute is not admitted in production mode |
| Brain | `BRAIN_PUBLIC_URL` | The same callback origin |
| Brain | `BRAIN_SECRET_REFS` | `{"local":{"worker":"MANAGED_WORKER_KEY"}}` plus other scoped references |
| Brain | `MANAGED_WORKER_KEY` | The worker's signing key, supplied through the environment, never arguments |
| Brain | `BRAIN_COMPUTE_WORKERS` | `{"local_compute":{"url":"http://127.0.0.1:8090/invoke","signing":"worker"}}` |

Start the worker binary separately from Brain. Register an installed harness in `BRAIN_HARNESSES`
and use a new `BRAIN_DATA` directory and an operator `BRAIN_API_TOKEN`. The official portable
harness lives in the Extensions repository; fixture harnesses are test-only.
Run `node examples/managed-package.mjs node` or `node examples/managed-package.mjs python`
with the same operator token and `BRAIN_URL` in the example's environment.

Managed fixtures build and run real locked npm/PyPI dependencies, exercise read-only/non-root
runtime limits, and recover stored receipts after deliberately lost transport. They do not
establish a production hostile-code boundary, live Modal/Fly access or published installation.

#### SQL control profiles

SQLite is the default. `BRAIN_CONTROL_URL` selects a PostgreSQL DSN when
`BRAIN_CONTROL=sql`. `tests\greenfield\postgres-emulator.ps1 -Cluster` creates a
disposable loopback PostgreSQL container and tests real Linux owner loss, retained
resource affinity and recovery. Build `.build\brain-linux` for `GOOS=linux`,
`GOARCH=amd64`, `CGO_ENABLED=0` before the cluster profile. The script removes only
its captured container, network and schema IDs.

The D1 profile uses the actual Miniflare/workerd D1 binding, not an in-memory mock:

```powershell
npm run build -w packages/brain-d1
npm test -w packages/brain-d1
$env:BRAIN_TEST_CONTROL = 'd1'
node --test --test-concurrency=1 tests\greenfield\journeys.test.mjs
```

After building `.build\brain-linux`, `tests\greenfield\d1-linux.ps1` runs the same
D1 components and public journeys in an isolated Linux container with immutable
Node/Python base images. This catches concurrency differences hidden by Windows
emulator timing. It creates a checkout-local source copy and removes its own
container, image tag and copied data after the run.

The required CI rollup also includes `session-server`: Go diagnostics, both SDK
journeys, real Docker-managed code, D1, PostgreSQL/Linux process faults, and native
S3/DynamoDB emulator conformance. The existing Rust, image and live-provider gates
remain separate and required when selected. Local/emulator success is not live
credential, immutable publication or production cutover evidence.

Run `pwsh -File tests/greenfield/archive-emulators.ps1` for real HTTP/S3 SDK and
DynamoDB Local conformance. The default S3 service is the official Moto **5.2.4**
Linux/amd64 image pinned by digest. It is an emulator, not evidence of live S3/R2
authorization or durability. The unchanged archive assertions also run against a
retained MinIO image with `-S3Engine minio`; this is a legacy comparison only.
MinIO's community repository is now unmaintained and its old public image is no
longer pullable, so CI must not depend on a workstation's cached copy.

Keep `TMP` and `TEMP` under the checkout before starting Miniflare. Its database
belongs outside Brain's retained data directory. `BRAIN_TEST_PYTHON` must select
the real interpreter when Windows exposes a launcher shim.

For a deployed Worker, set `BRAIN_CONTROL=d1`, `BRAIN_CONTROL_URL` to its HTTPS
origin and `BRAIN_D1_KEY` to the same retained 32-byte hexadecimal service key
configured as a Worker secret. This is separate from the public API token and
cluster custody key. Set a real database binding in `packages/brain-d1/wrangler.jsonc`
before deploying; the checked-in zero database ID is deliberately not deployable.
Do not point a conformance job at a production database.

`npm run test:live -w packages/brain-d1` requires `BRAIN_D1_ACCOUNT_ID` and
`CLOUDFLARE_API_TOKEN`. It creates and deletes one disposable database and tests
actual cloud limits. Missing credentials fail, rather than skip. Miniflare
currently accepts a row beyond the documented cloud limit, so the local gate is
not evidence of cloud limit enforcement. The Worker independently rejects
values above one MiB and bounds request bytes, reads, parameters and batch size.

Check that generation leaves no uncommitted changes in the generated contracts, SDK types or
configuration reference. CI also runs real Tool worker and HTTP lifecycle integration tests, the locked Python
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

Upstream adapter and runtime tests live in the extension repository. Brain tests its neutral
protocol with independent JavaScript and Python implementations, including mixed worker pools.

Main CI publishes immutable candidate images. Only release promotion moves the image and npm
`latest` tags, after the media gate passes.

Publication and registry verification are separate jobs. Verification allows up to five minutes
for registry visibility and rejects conflicting archive integrities or authentication failures.
Server-only releases may reuse an identical SDK archive. Verification checks its original signed
Brain release provenance and exact archive digest; it never republishes that package version.
If verification fails after an upload, re-run the failed jobs in the same workflow run: they
download the original archives and do not publish again.

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

Keep complete runnable programs in [`examples/`](examples). Task excerpts can show the relevant
code with a link to the full program and explicit prerequisites. Reuse the canonical setup rather
than maintaining another complete program. Follow the [writing guide](references/documentation.md).

## Conventions

- Commit messages are `area: imperative summary`.
- Fail fast. Keep comments self-contained. Write plain English.
- Journal every effect before it happens, and record terminal results before the loop sees them.
- Keep the `brain` crate free of cloud SDKs. Storage, custody, and runtime behaviour go behind
  public adapters.
- Do not weaken a production invariant to make local development easier.

[`AGENTS.md`](AGENTS.md) has the full working rules for this repository.
