# Harness integration and prompt caching notes

- **Reviewed:** 2026-10-15
- **Context:** Background research for [ADR 0001](adr/0001-local-first-session-server.md) and the [harness adapter plan](plans/03-harness-adapters.md).

## Keep the public model small

Brain exposes sessions and events (`user.message`, `assistant.message`, `assistant.delta`, `tool.call`, `tool.progress`, `tool.result`, `session.status`, `session.error`)—similar in spirit to [Claude Managed Agents](https://platform.claude.com/docs/en/managed-agents/reference), but without copying a separate permission, environment, or workflow taxonomy.

Accepting an input, writing it to disk, updating model context, and running inference are distinct steps. Brain acknowledges queued input (`202 Accepted`) as soon as it is recorded locally, then streams or records execution events as the harness processes the turn.

## Responsibility boundaries

| Concern | Owner |
| --- | --- |
| Auth, tenant scope, input queueing, local storage, background checkpoint sync, runner process lifecycle | Brain |
| Prompt construction, context compaction, tool call scheduling, continuation after tool results | Harness |
| Tool execution, business approvals, external jobs, sandboxes, resource cleanup | External tool service |
| Prompt cache matching, placement, and TTL | LLM provider |
| Routing, sticky session placement, scaling, failover | Host platform |

Brain authenticates every caller and stamps `tenant_id`, `session_id`, and `call_id` onto outbound tool requests. A tool response is always wrapped inside a `tool.result` envelope—it can never inject a fake `session.status` or `system.message` event.

## Waiting on tools vs. returning job handles

Making an HTTP handler async does not make the agent loop non-blocking: if the harness is awaiting the tool's final response, the model cannot do anything else until the tool returns. Two straightforward patterns cover long-running work:

1. **Hold the request open (HTTP or SSE):** The tool streams optional `progress` frames and finishes with a `result` frame within `timeout_seconds`. Brain stays responsive to SSE observers and `user.interrupt` calls while waiting.
2. **Return a job handle immediately:** The tool returns a job ID right away, completing the tool call so the model can continue or call separate `status` / `cancel` tools later.

Human approvals work with either pattern. The tool service performing the sensitive action must enforce that approval was granted; simply instructing the model to "ask first" is not an access control boundary.

### No synthetic heartbeats or cache warming

Brain never sends periodic status prompts or synthetic heartbeats to keep a waiting model active or keep provider caches warm:
- **Pi** defaults `cacheWarming` to `"streaming"`, so Brain's Pi runner explicitly sets `cacheWarming: "off"`. ([Pi settings](https://raw.githubusercontent.com/badlogic/pi-mono/main/packages/coding-agent/docs/settings.md))
- **OpenCode** disables warming by default; Brain's planned adapter will also explicitly set `warming: false`. ([OpenCode warming](https://opencode.ai/v2/docs/warming))

## Upstream harness comparison

| Harness | Upstream control surface | Integration approach |
| --- | --- | --- |
| **Pi** | TypeScript SDK (`SessionManager`, custom tools, model/thinking settings, `steer`/`followUp`, `abort`, lifecycle events). ([Pi SDK](https://raw.githubusercontent.com/badlogic/pi-mono/main/packages/coding-agent/docs/sdk.md)) | Thin Node.js runner over stdio (`runners/pi`). Disables built-in local tools, extensions, and skills; bridges selected tools to Brain over stdio. |
| **Codex** | App Server over stdio (`turn/start`, `turn/steer`, `turn/interrupt`, resume/fork/revert, MCP). ([Codex App Server](https://learn.chatgpt.com/docs/app-server)) | Planned stdio adapter. Needs verification that built-in local tools can be fully excluded and native state restored passively. |
| **OpenCode** | HTTP server & v2 plugin hooks (queue/steer delivery, model switching, context/tool hooks). ([OpenCode v2 API](https://dev.opencode.ai/v2/docs/api), [plugins](https://opencode.ai/v2/docs/build/plugins/)) | Planned adapter. Must isolate per-session state and prevent OpenCode's startup recovery from automatically resuming sessions on boot ([session spec](https://github.com/anomalyco/opencode/blob/dev/specs/v2/session.md)). |

## Common baseline across harnesses

Rather than locking Brain to the lowest common denominator forever, every harness adapter must meet a strict baseline while optional features (`steer`, live config updates, rewind) fail with `422 unsupported` when unavailable:

- **Input during active turns:** Queue incoming messages in order (`delivery: "queue"`). Only expose `steer` when the adapter has a tested native boundary.
- **Session config:** Fixed at creation for launch; see [proposed configuration updates](plans/05-api-and-sdks.md#future-configuration-updates-proposed) for how idle-boundary updates will work later.
- **Tool isolation:** Every built-in local execution tool (bash, file edit, ambient MCP) must be disabled so the model can only call Brain-bound tools.
- **Passive recovery:** Restoring a session loads native files at the last completed turn boundary without triggering inference or replaying tools.

## Recovery vs. provider prompt caching

Restoring a session and hitting a provider's prompt cache are two different things:
- **Session recovery** restores the harness's native conversation state (compaction summaries, active branches, tool call/result pairs) so the next turn sends the exact intended prompt.
- **Prompt caching** ([OpenAI](https://developers.openai.com/api/docs/guides/prompt-caching), [Anthropic](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)) is a provider-side optimization that saves prefill latency and cost when the prompt prefix matches a warm cache entry. Even a perfectly restored session will miss the provider cache if its TTL expired.

What Brain does to stay cache-friendly:
1. Keeps system instructions, model settings, and tool definitions in a stable order for the life of the session (avoiding per-turn timestamps or changing paths in prefixes).
2. Restores the harness's own native state files rather than rebuilding context from a flattened transcript.
3. Avoids hashing the entire transcript on every turn—Brain's append-only log offsets already track local state changes, and only the LLM provider knows whether its internal KV cache hit ([OpenAI cache diagnostics](https://developers.openai.com/api/docs/guides/prompt-caching/diagnostics)).
