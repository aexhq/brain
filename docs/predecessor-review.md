# Lessons from the predecessor runtime

- **Date:** 2026-10-15
- **Context:** Architectural review comparing the previous Rust/Wasm Brain prototype ([ADR index][old-index]) with the current Go implementation ([ADR 0001](adr/0001-local-first-session-server.md)).

## Summary of changes

The current Brain server keeps the predecessor's best ideas—single-owner sessions, passive recovery, code-generated contracts, and strict error separation—while dropping its custom Environment framework, in-process extension runtime, and synchronous per-effect disk flushes.

| Area | Predecessor (ADRs 009–057) | Current Brain ([ADR 0001](adr/0001-local-first-session-server.md)) |
| --- | --- | --- |
| **Durability** | Synchronous `fsync` before every effect and publication ([ADR-032][old-032], [ADR-035][old-035]). | Append locally first; upload checkpoints asynchronously in 64 KiB blocks. Machine loss can lose the unsynced tail, but known local write failures immediately abort the turn. |
| **Storage layout** | Single canonical execution journal ([ADR-032][old-032]). | SQLite for session metadata; append-only event log for history; native harness files for model context. |
| **Tool retries** | Never automatically retry model, tool, or resource effects ([ADR-035][old-035]). | Same rule for tools: Brain never replays tool calls. Only idempotent background checkpoint uploads are retried. |
| **Model & tool mediation** | Every model and tool call passed through Brain's own effect proxy ([ADR-053][old-053]). | Harnesses call LLM providers directly; Brain mediates all tool calls over HTTP/SSE and disables built-in local harness tools. |
| **Event identity** | `(session_id, sequence)` only ([ADR-040][old-040]). | Opaque event IDs and cursors so events created after a lost-tail restore cannot collide with lost IDs. |
| **Approvals** | Handled by external application tools ([ADR-045][old-045]). | Same: approvals are ordinary HTTP/SSE tools ([`examples/tools`](../examples/tools/README.md)). |
| **Multi-tenancy** | Single deployment bearer token; tenancy enforced upstream ([Product review][product-review]). | Tokens map to explicit tenant/session grants (`manage`, `read_only`) enforced by Brain on every route. |
| **Tool completion** | Required an explicit `finish` call after returning ([ADR-048][old-048]), which caused ordinary lookups to hang and time out ([interface experiments][interface-research]). | Returning an HTTP JSON response (or final SSE `result` frame) completes the tool call immediately ([app integration ADR][app-integration]). |

## Key engineering lessons applied in Brain

1. **Don't hold locks across model or tool waits.**
   The predecessor hit deadlocks when a global session table lock was held during recovery and when active turns consumed the same permit pool needed by their own tool callbacks ([ADR-010][old-010], [ADR-037][old-037], [Sept 5 review][review-sep05]). Brain locks only brief in-memory state transitions and never blocks tool completions or interrupts behind new-turn admission.
2. **Keep request acceptance separate from turn completion.**
   In the predecessor, an overload rejection consumed the caller's idempotency key, and a failed turn could resolve `send()` as a normal `idle` summary ([interface experiments][interface-research]). In Brain, rejected requests (`429`) never consume an idempotency `key`, `send()` returns the accepted `user.message` event (`202`), and turn completion is reported through `session.status` and `session.error`.
3. **Test recovery by inspecting the next model request.**
   Comparing serialized JSON files missed bugs where restored sessions sent empty or altered prompts on the next turn ([native-state research][native-research]). Brain's restore tests run a recording LLM provider and assert that the next Pi model request after a cold restore preserves the prior conversation prefix.
4. **Avoid quadratic checkpoint writes.**
   Re-saving or re-uploading an entire conversation after every turn creates $O(N^2)$ write amplification ([ADR-010][old-010], [ADR-019][old-019], [performance verdicts][perf-verdicts]). Brain uploads immutable 64 KiB blocks and reuses already uploaded blocks on later checkpoints.
5. **Distinguish torn final records from interior corruption.**
   A corrupted length header in the middle of a file must not be silently truncated as if it were an incomplete tail from a crash ([Sept 21 review][review-sep21]). Brain trims only an incomplete final record at startup and fails loudly on interior file corruption until an explicit `restore` is run.
6. **Enforce size limits while reading, before allocating.**
   Checking payload limits after buffering the full body or allowing unbounded SSE subscriber queues caused OOMs ([Sept 5 review][review-sep05], [Sept 21 review][review-sep21], [ADR-043][old-043]). Brain caps HTTP request bodies, runner stdio frames (4 MiB), tool responses (2 MiB), and SSE subscriber buffers while reading.
7. **Keep tool payloads inside a result envelope.**
   Allowing raw tool JSON to share top-level protocol fields caused a tool returning `{"status": "cancelled"}` to trigger a protocol error ([ADR-047][old-047], [interface experiments][interface-research]). Brain wraps all tool outputs inside `tool.result` (`content`, `is_error`, `unknown_outcome`) so business fields can never collide with Brain control state.
8. **Silent tool waits are not dead runners.**
   Killing a runner because an SSE tool waited quietly for human approval broke long-running tools ([Sept 21 review][review-sep21]). Brain bounds tool waits with per-tool `timeout_seconds` rather than killing the harness process on quiet streams.


[old-index]: ../../../aex_workspace/brain/references/adrs/README.md
[old-009]: ../../../aex_workspace/brain/references/adrs/2026-08-28-01-write-behind.md
[old-010]: ../../../aex_workspace/brain/references/adrs/2026-08-28-02-growth.md
[old-012]: ../../../aex_workspace/brain/references/adrs/2026-08-30-01-provider-model.md
[old-015]: ../../../aex_workspace/brain/references/adrs/2026-08-31-01-telemetry.md
[old-018]: ../../../aex_workspace/brain/references/adrs/2026-09-02-01-presentation.md
[old-019]: ../../../aex_workspace/brain/references/adrs/2026-09-02-02-prefix-deltas.md
[old-026]: ../../../aex_workspace/brain/references/adrs/2026-09-04-04-load-recovery.md
[old-030]: ../../../aex_workspace/brain/references/adrs/2026-09-04-08-clean-break.md
[old-031]: ../../../aex_workspace/brain/references/adrs/2026-09-04-09-rust-contracts.md
[old-032]: ../../../aex_workspace/brain/references/adrs/2026-09-05-01-canonical-journal.md
[old-035]: ../../../aex_workspace/brain/references/adrs/2026-09-05-04-send-once.md
[old-036]: ../../../aex_workspace/brain/references/adrs/2026-09-05-05-interruption.md
[old-037]: ../../../aex_workspace/brain/references/adrs/2026-09-05-06-ephemeral.md
[old-039]: ../../../aex_workspace/brain/references/adrs/2026-09-05-08-benchmark-policy.md
[old-040]: ../../../aex_workspace/brain/references/adrs/2026-09-05-09-session-names.md
[old-043]: ../../../aex_workspace/brain/references/adrs/2026-09-06-02-deployment-limits.md
[old-045]: ../../../aex_workspace/brain/references/adrs/2026-09-07-01-protocol-freeze.md
[old-047]: ../../../aex_workspace/brain/references/adrs/2026-09-12-01-tool-outcomes.md
[old-048]: ../../../aex_workspace/brain/references/adrs/2026-09-21-01-tool-completion-and-event-activation.md
[old-050]: ../../../aex_workspace/brain/references/adrs/2026-09-23-02-tool-context-and-model-boundary.md
[old-052]: ../../../aex_workspace/brain/references/adrs/2026-09-28-01-client-connection-lifecycle.md
[old-053]: ../../../aex_workspace/brain/references/adrs/2026-10-14-01-startup-harness.md
[old-056]: ../../../aex_workspace/brain/references/adrs/2026-10-14-02-registered-agentloops.md
[old-057]: ../../../aex_workspace/brain/references/adrs/2026-10-14-03-default-agentloops.md
[sdk-lifecycle]: ../../../aex_workspace/aex-research/docs/adr-2026-09-12-sdk-lifecycle-and-tool-execution.md
[app-integration]: ../../../aex_workspace/aex-research/docs/adr-2026-09-24-app-integration-and-tool-status.md
[interface-research]: ../../../aex_workspace/aex-research/docs/brain-interface-research-2026-09-24.md
[native-research]: ../../../aex_workspace/aex-research/docs/brain-findings.md
[perf-verdicts]: ../../../aex_workspace/aex-research/docs/brain-performance-verdicts-opus-2026-08-28.md
[review-sep05]: ../../../aex_workspace/aex-research/docs/brain-architecture-review-2026-09-05/README.md
[review-sep21]: ../../../aex_workspace/aex-research/results/2026-09-21-brain-review/report.md
[product-review]: ../../../aex_workspace/aex-research/docs/brain-product-review-2026-09-27.md
