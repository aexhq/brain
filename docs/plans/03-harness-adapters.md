# 03: Harness adapters plan

- **Releases:** Pi at R0; OpenCode and Codex at R2 ([Roadmap](../roadmap.md))
- **Background:** [Harness integration notes](../harness-integration-notes.md)
- **Integration requirement:** [ADR 0004](../adr/0004-service-integration-readiness.md) requires Pi, Codex and OpenCode before service adoption. [Plan 12](12-service-integration-readiness.md) adds private model credentials, common host ownership and model-request evidence to the conformance gates below.
- **Implementation evidence:** Plan 12 implements all three native adapters and records their local conformance, compaction, recovery, credential and incremental-state checks. The sections below describe the adapter contract; the qualification record separates local results from live providers.

Wrap each agent harness behind `harness.Factory` and `harness.Runner` so Brain owns HTTP admission, storage, and tool dispatch while the harness owns prompt construction, model calls, and context compaction.

## Work items

| ID | Scope | Acceptance criteria |
| --- | --- | --- |
| **H1: Runner contract** | Define `Factory` / `Runner` (`Open`, `Prompt`, `Steer`, `Interrupt`, `Checkpoint`, `Close`) and the newline-delimited JSON stdio protocol (`RunnerFrame`). | Protocol frames are capped at 4 MiB; tool errors return as `tool.result` while broken stdio or local append failures abort the turn. |
| **H2: Pi adapter** | Implement `runners/pi` wrapping `@earendil-works/pi-coding-agent` over stdio with `cacheWarming: "off"` and all built-in local tools/extensions disabled. | Multi-turn conversations, custom HTTP/SSE tools, interruption, and cold checkpoint restore pass against the recording LLM provider. |
| **H3: OpenCode adapter** | Wrap the pinned OpenCode v2 runtime with isolated state, `warming: false`, remote tool bindings, and suppressed startup auto-resume. | Passes the shared harness conformance suite without leaking tools/config across sessions or waking sessions on open. |
| **H4: Codex adapter** | Drive Codex App Server over stdio, mapping native threads/turns and binding external tools via MCP or tool callbacks. | Passes the shared conformance suite with built-in local execution tools disabled and passive native restore verified. |
| **H5: Process residency** | Load runners lazily on turn execution; evict idle runners when `loaded` limits are reached; measure memory and checkpoint growth across long sessions. | `GET /sessions/{id}` and `GET /sessions/{id}/events` never spawn a runner; evicted sessions reload cleanly from their saved checkpoint on the next message. |

## Adapter requirements

1. **No background model calls:** Opening a saved session, sitting idle, or receiving SSE keepalives/health checks must never trigger an LLM request.
2. **No local tool bypass:** Built-in bash, file-editing, or ambient plugin tools must be disabled so the model can only call tools listed in the session's `config.tools`.
3. **Verified next-turn context:** Conformance tests must restore a saved checkpoint into a fresh directory, send a follow-up prompt against a recording provider, and verify that the outbound model request preserves prior conversation context.
