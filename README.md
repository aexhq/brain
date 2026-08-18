# brain

The **brain**: the aex LLM harness. One long-lived process owns every session's decisions —
the sealed prefix, the provider round, the tool loop, the journal (one durable write per
decision), the SSE event stream, admission and cancellation.

The core is **substrate-generic and composable**: where tools run, where the journal
persists and where keys rest are adapters behind public traits (`brain::adapter`,
`brain::journal::JournalStore`, `brain::keys::KeyCustody`). Built-ins: a zero-config local
substrate (this crate) and AWS (`brain-aws`: DynamoDB + KMS + Lambda MicroVMs). A custom
substrate — a k8s pod, an SSH box, another cloud — implements the traits and composes via
`Brain::with_parts`; no core change, no fork. Wire formats live in
[`aexhq/aex`](https://github.com/aexhq/aex); the MicroVM guest lives in
[`aexhq/hands`](https://github.com/aexhq/hands); both consumed by tag.

| Crate / module | What |
| --- | --- |
| `brain::adapter` | **the seams**: `HandAdapter`/`HandFactory` (tool execution + workspace lifecycle), plus `JournalStore` and `KeyCustody` |
| `brain::provider` | the `Provider` seam: Anthropic Messages + OpenAI Chat Completions dialects, streamed over a tested SSE decoder; raw usage read, absent ≠ zero |
| `brain::config` | the sealed prefix: type-enforced immutability, digest includes tool order |
| `brain::journal` | journal semantics (fence on claim only, (session, seq) idempotency barrier, fold/replay) + the in-memory reference store |
| `brain::local` | the local substrate: the seven manifest tools in subprocesses against per-session directories |
| `brain::turn` | the tool loop: journal-before-dispatch, bounded-parallel adapter calls, bounded tail-retained output, graceful cancel |
| `brain::session` | sessions as spawned tasks: hydrate-act-commit-discard actors, admission, `malloc_trim` on drop; `Brain::with_parts` composition |
| `brain::api` | session API v1 (axum): create/message/events(SSE)/cancel/end/delete/persist/artifacts |
| `brain-aws` | the AWS adapter set: `DynamoJournal`, `KmsCustody`, `LambdaFactory` (MicroVM launch/resume/sync/wall survival) |
| `brain-server` | the composed binaries: `brain` (local by default, AWS by `AEX_MODE=aws`) and `m0` (the AWS gate) |

## Run it locally (the default)

```
cargo run --bin brain
```

That is the whole setup: `AEX_MODE=local` is the default — in-memory journal, tools executed
locally in subprocesses against per-session directories under `./aex-data`, the session API on
`127.0.0.1:8700` (a bearer token is generated and printed if `AEX_API_TOKEN` is unset). Bring
your own model key per session (`model.api_key`, plus `model.base_url` for any
OpenAI/Anthropic-compatible endpoint).

Two things local mode is honest about:
- **not durable** — the journal is in memory; sessions do not survive a restart (workspaces
  on disk do);
- **not a sandbox** — subprocesses are process separation only. Run prompts you trust.
  Production isolation is `AEX_MODE=aws`: every session in its own AWS Lambda MicroVM
  (Firecracker), DynamoDB journal, KMS key custody, S3 workspace sync. See `bin/brain.rs`
  for the aws-mode environment.

| Mode | journal | tools | custody | storage |
| --- | --- | --- | --- | --- |
| `local` (default) | in-memory | subprocesses, per-session dir | in-memory | local dirs |
| `aws` (production) | DynamoDB (lease + fence) | Lambda MicroVM per session | KMS | S3, presigned |

The session API is identical in every composition — that is the point of contracts-first.

## Write your own adapter

Implement `brain::adapter::{HandFactory, HandAdapter}` (and optionally
`journal::JournalStore` / `keys::KeyCustody`), then compose:

```rust
let brain = Brain::with_parts(
    BrainConfig::default(),
    Journal::new(Arc::new(MyJournalStore::new(...)), owner),   // or Journal::new_memory(owner)
    Arc::new(MyCustody::new(...)),                             // or PlainCustody (dev only)
    Arc::new(MyHandFactory::new(...)),                         // your substrate
    None,                                                      // default provider dialects
);
brain::api::serve(AppState { brain, token }, addr).await?;
```

`tests/custom_adapter.rs` is the complete living example: a third-party substrate written
against nothing but the public API, driven over real HTTP, including seed staging, streamed
tool output, artifact URLs and purge-on-delete. The contract every adapter must hold is
documented on the traits; the shared journal tests define the store semantics.

## Build and test

```
cargo test --workspace                              # unit + local-mode e2e + leakage gate (no cloud)
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p brain-bench --release -- ci            # the benchmark gates (density arm needs Linux)
cargo run --bin m0        # the AWS-mode M0 gate; needs AWS + provider keys (bin/m0.rs header)
```

The published numbers — platform-added TTFT, turns/s, KiB per resident session, reclaim,
cross-session leakage, in-region hand latency, no-IMDS — live in [BENCHMARKS.md](BENCHMARKS.md)
with the method that produced them. The same suite gates every push.

License: Apache-2.0.
