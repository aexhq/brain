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

## Build and test

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --bin m0        # needs AWS + provider keys; see bin/m0.rs header
```

License: Apache-2.0.
