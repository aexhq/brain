# brain

The **brain**: the aex LLM harness. One long-lived process owns every session's decisions —
the sealed prefix, the provider round, the tool loop over the brain↔hand ABI, the DynamoDB
journal (one durable write per decision), the SSE event stream, admission and cancellation.
Hands (tool execution in AWS Lambda MicroVMs) live in [`aexhq/hands`](https://github.com/aexhq/hands);
wire formats live in [`aexhq/aex`](https://github.com/aexhq/aex) and are consumed by tag.

| Module | What |
| --- | --- |
| `provider/` | the `Provider` seam: Anthropic Messages + OpenAI Chat Completions dialects, streamed over a tested SSE decoder; raw usage read, absent ≠ zero |
| `config` | the sealed prefix: type-enforced immutability, digest includes tool order |
| `journal` | DynamoDB journal: one item collection per session, lease + fence, `attribute_not_exists` idempotency, fold/replay |
| `turn` | the tool loop: journal-before-dispatch, parallel batches over ephemeral lanes, bounded tail-retained output, graceful cancel |
| `hand` | brain-side hand policy: launch/hello/reconnect, keepalive, speculative resume, turn-end sync, wall-survival re-materialise |
| `session` | sessions as spawned tasks: hydrate-act-commit-discard actors, admission, `malloc_trim` on drop |
| `api` | session API v1 (axum): create/message/events(SSE)/cancel/end/delete/persist/artifacts |
| `bin/brain` | the server |
| `bin/m0` | the M0 gate: the full arc against real provider keys and real MicroVMs |

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

The session API is identical in both modes — that is the point of contracts-first.

## Build and test

```
cargo test --workspace                              # unit + the local-mode e2e (no cloud)
cargo clippy --workspace --all-targets -- -D warnings
cargo run --bin m0        # the AWS-mode M0 gate; needs AWS + provider keys (bin/m0.rs header)
```

License: Apache-2.0.
