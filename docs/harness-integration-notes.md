# Harness control, tool continuation, and recovery

Reviewed: 2026-10-15  
Status: Research supporting [ADR 0001](adr/0001-local-first-session-server.md); integrations are planned, not implemented.

## Keep the public model small

Use sessions and events with familiar message/tool/status concepts. Claude Managed Agents provides a useful precedent: user.message, agent.message, tool-use/result events, session status, and stream-only deltas. Its system.message is privileged and model-dependent. Brain can adopt the interaction style without copying its permission, environment, workflow, or agent-resource model. [Claude event reference](https://platform.claude.com/docs/en/managed-agents/reference)

Sending an event, storing history, changing model-visible context, and starting inference are different operations even when the public API is simple. Brain should report accepted input and actual execution outcome separately without requiring a public run/effect/job taxonomy. Initial instructions belong in session configuration. Append-only system messages can be added only when an adapter genuinely supports them.

## Responsibility boundaries

| Concern | Owner |
| --- | --- |
| Session authentication, tenant scope, event identity, input acceptance, bounded I/O, local persistence, external sync, own runner lifecycle | Brain |
| Next model request, message selection, native context/compaction, tool scheduling, queued input/steering, and continuation policy | Harness |
| Model decisions after receiving a result | Model within the harness loop |
| Tool execution, business authorization/approvals, remote jobs, resource limits, artifacts, and cleanup | External tool service |
| Cache matching, placement, retention, and cached-input accounting | LLM provider |
| Placement, cross-instance ownership, global quotas, failover | Host platform |

Brain must authenticate the source of events it accepts. A tool result cannot declare itself a system message or internal checkpoint. This only needs scoped endpoints/call correlation and typed internal boundaries, not a public provenance framework. The harness then controls how accepted content enters context. Distribution removes Brain's compute-resource lifecycle, but not its responsibility for its own files, queues, processes, and connections.

## Tools and responsiveness

An asynchronous handler does not necessarily make the agent nonblocking. If the harness awaits that handler's final result, the model cannot notice the delay and decide to poll until the harness invokes it again. Progress events also do not necessarily become model context.

Two ordinary patterns suffice:

- **Wait for a result.** HTTP or SSE stays open while the tool works. Brain can still handle observation and interruption; model continuation depends on the harness.
- **Return a handle.** The tool invocation completes promptly with a job/request ID. The harness can continue, and the model may call status/cancel tools or do other work. The job belongs to the external service. A later notification starts or steers work only through a tested harness input path.

An approval tool can use either pattern. The action service must enforce its approval requirement; a model instruction to ask first is not enforcement. Brain needs no approval database, decision endpoint, or gate.

Do not keep an otherwise waiting model active with synthetic heartbeats, periodic status prompts, or cache-warming requests. Transport pings and health checks must not enter context or start inference. Observed polling differences between products depend on their tool protocol and harness policy; they are not guarantees about the underlying model. A model can still choose to poll after an ordinary tool returns a handle.

Explicit adapter settings matter: Pi currently defaults `cacheWarming` to `"streaming"`, which can warm caches during active runs; set it to `"off"`. OpenCode warming is disabled by default; set `warming: false` in Brain's controlled configuration. Neither setting is a new public Brain concept. [Pi settings](https://raw.githubusercontent.com/badlogic/pi-mono/main/packages/coding-agent/docs/settings.md), [OpenCode warming](https://opencode.ai/v2/docs/warming)

For comparison, Claude Managed Agents documents blocking custom-tool calls that pause until their results arrive. Its event API alone does not make every call nonblocking. [Managed tool-result flow](https://platform.claude.com/docs/en/managed-agents/events-and-streaming#answer-tool-calls-that-pause-the-session)

Tool failure, denied approval, transport timeout, and unknown remote outcome are tool results. Brain does not choose whether the model retries or gives up. A failed Brain append or broken runner protocol is different: stop the affected execution through a control path, even if the harness normally catches tool exceptions. Brain itself does not replay tool calls. Timeouts release bounded resources; they do not establish that an external action stopped.

## Integration surfaces

These are current documented upstream surfaces, not conformance claims for Brain. Pin and test the actual dependency selected during implementation.

| Harness | Control available | Proposed integration and limits |
| --- | --- | --- |
| Pi | SDK access to tools, configuration, messages, SessionManager, steer/followUp, abort, and lifecycle events. SessionManager owns the active branch and compaction; assigning agent.state.messages does not replace persisted context. | Thin TypeScript SDK runner with remote custom-tool wrappers. Pi supplies the loop and native session store. Use its settled boundary for completion and native branch operations for rewind. [SDK](https://raw.githubusercontent.com/badlogic/pi-mono/main/packages/coding-agent/docs/sdk.md) |
| Codex | App Server exposes resume/fork/revert, turn input/steering/interrupt, standalone tool output, and history injection. Dynamic tool callbacks are still experimental; MCP is another integration path. | Supervised App Server over stdio. Preserve native state; use supported remote tools. Injection without a new turn and submitting tool output are distinct operations. Prove tool exclusion and native restore before advertising them. [App Server](https://learn.chatgpt.com/docs/app-server) |
| OpenCode | The current upstream v2 interface includes explicit queue/steer delivery, session model switching, instruction entries, interruption, and context inspection. | Supervised server with controlled configuration/state and trusted hooks where needed. Experimental transcript import/export does not establish full native recovery. [API](https://dev.opencode.ai/v2/docs/api) |

The earlier assessment based on OpenCode's older server page understated its controls. Current v2 plugins can transform request context and tools; each request captures executable tool definitions. Request-only context edits are not persisted history, so restoration must reestablish the adapter configuration. Use these hooks before considering source patches. [OpenCode v2 plugins](https://opencode.ai/v2/docs/build/plugins/)

OpenCode's development session contract also describes startup recovery that resumes claimed sessions automatically, while tools/plugins/model resolution share a Location scope. Brain must isolate that scope and suppress unsolicited activation. This is an implementation risk to test against the chosen build, not a reason to assume transcript import is passive native recovery. [Session contract](https://github.com/anomalyco/opencode/blob/dev/specs/v2/session.md)

Pi exposes request-context transforms, model/thinking selection, and activation of registered tools. Its structured prompt updates can preserve earlier prefixes where supported. Codex supports model/effort overrides on `turn/start`, but `turn/steer` cannot carry these overrides. Codex dynamic tools are experimental and supplied at thread setup; a configuration-wide MCP reload is not an established atomic, session-scoped replacement operation. [Pi extensions](https://raw.githubusercontent.com/badlogic/pi-mono/main/packages/coding-agent/docs/extensions.md), [Codex App Server](https://learn.chatgpt.com/docs/app-server)

All three can include native tools or ambient configuration. An adapter must prove that the selected setup cannot bypass Brain's remote bindings. Configuring native permissions is adapter setup, not a Brain approval subsystem. Unexpected native approval requirements must be surfaced as an unsupported setup, not automatically approved. Complete exclusion of native execution and portable restore remain implementation gates, especially for Codex/OpenCode; documentation review alone does not prove them.

## A common interface without a permanent feature ceiling

No harness is the most restrictive in every dimension. Define shared behavior precisely and add only the optional operations that real integrations need:

| Boundary | Common promise | Restriction and approach |
| --- | --- | --- |
| Busy input | Accept bounded queued input for delivery after settled work; expose inspection and interruption independently. | Steering is optional and uses the native boundary. Pi steering waits for the current assistant turn and its tools. Never claim immediate model attention or silently substitute queueing. [Pi SDK](https://raw.githubusercontent.com/badlogic/pi-mono/main/packages/coding-agent/docs/sdk.md) |
| Configuration | One configuration shape, frozen at launch. | Start future edits at idle boundaries. Model/effort changes are easier to unify than live instruction/tool replacement. See the [configuration proposal](plans/05-api-and-sdks.md#future-configuration-updates-proposed). |
| External tools | Explicit schemas and correlated results through selected remote bindings. | Disabling all bypassing native tools is mandatory. Live tool replacement is optional; do not reduce everything to a generic dispatcher tool to hide a missing registry hook. |
| Context | Familiar message/tool events plus opaque native saved state. | Arbitrary history replacement is not a portable operation. Hooks and raw history injection have different persistence and authority semantics. |
| Recovery | Passive restore at a completed, saved boundary; no automatic replay. | Cross-harness history transfer and cold continuation of arbitrary pending tools are outside the baseline. Native recovery must not wake a model by itself. |
| Interruption | Deliver the cancellation request and report the observed outcome. | Stopping the local wait does not cancel or roll back external work. The tool service owns external cancellation. |

For future configuration, use an immutable value per model request and retain its tool bindings for the calls that request creates. Do not make all configuration permanently constructor-only. Keep lifecycle ownership and harness identity fixed, but allow the adapter to acquire the next configuration at a proven boundary. Brain needs no prompt-diff engine, config revision, or second model loop.

Prefer supported APIs, then trusted adapter hooks, then a small patch for an important missing seam. Suitable patch targets are tool registration/dispatch, fatal-stop propagation, passive restore, and pre-request settings. Pin the dependency and require behavioral tests. Reimplementing the loop, modifying native history directly, or maintaining old API variants would recreate the predecessor's complexity. The [harness plan](plans/03-harness-adapters.md) owns these gates.

## Recovery and prompt caching

Recovery preserves the conversation's meaning and usable native state. A normalized transcript can omit active-branch selection, compaction state, call/result relationships, media representation, and provider continuation items. Preserve those through the harness's own persistence. No live process snapshot or portable provider KV cache is implied.

Prompt caching saves input-prefix computation on the next inference. A miss can increase input cost and latency; it does not re-run earlier model generations or tools. Starting a new session with identical context still has to process that context; starting without it changes what the model knows. A provider cache can expire even when recovery is correct. [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching)

Changing initial tools/instructions or model settings can affect cache reuse, but not every change invalidates everything. Current OpenAI documentation describes configuration updates that preserve earlier prefixes on supported models. Claude documents model-dependent effort effects and supported append-only tool/effort changes. These are provider/harness capabilities, not reasons to require mutable Brain configuration at launch. [OpenAI caching settings](https://developers.openai.com/api/docs/guides/prompt-caching#which-settings-affect-the-cached-prefix), [Claude cache invalidation](https://platform.claude.com/docs/en/build-with-claude/prompt-caching#what-invalidates-the-cache)

OpenAI Agents SDK separates conversation persistence from resumable execution state: applications can use local sessions/history or provider-side continuation, and interrupted runs expose a state snapshot. Its public contract does not equate these with a cache hit. Claude Managed Agents retains session history and controls continuation itself; clients interact through events. [SDK conversation strategies](https://developers.openai.com/api/docs/guides/agents/running-agents#choose-one-conversation-strategy), [SDK interrupted state](https://developers.openai.com/api/docs/guides/agents/results#interrupted-runs-return-state-not-a-final-answer), [Managed session continuation](https://platform.claude.com/docs/en/managed-agents/events-and-streaming#resume-an-idle-session)

Recommendation for Brain:

1. Freeze Brain-controlled settings and selected tool definitions/bindings for launch. Persist them once and preserve their order. Future updates should use tested boundaries and supported native prompt changes. Do not insert changing timestamps or paths into earlier instructions.
2. Let the harness compact and construct requests. Restore the same native context; compare the next request in adapter tests for lost semantics or accidental prefix changes.
3. Do not hash full transcripts/prompts each turn. Immutable configuration and append positions already describe Brain's own changes. A hash of Brain's projection cannot prove the provider-rendered prefix or cache residency, so it would not drive a reliable action.
4. Observe cached-input usage when exposed. Add opt-in request diagnostics only for a demonstrated problem. Do not add a Brain cache warmer or compulsory model proxy.

The reviewed public contracts do not require an application-side full-history hash. OpenAI now exposes provider-side cache diagnostics and cached-token usage; use them if the harness makes them available. Its diagnostics use internal hashes, which is different from asking Brain to hash a normalized transcript. [Cache diagnostics](https://developers.openai.com/api/docs/guides/prompt-caching/diagnostics)

An upstream harness may hash its own state: OpenCode's current session contract hashes instruction content to track updates. That does not require Brain to duplicate the work or hash the entire growing transcript. [Instruction synchronization](https://github.com/anomalyco/opencode/blob/dev/specs/v2/session.md)

## Append-only storage and rewind

Keep retained historical records immutable. Compaction changes the active context, not the historical facts. Rewind selects an earlier boundary and appends subsequent work from there; it does not modify earlier message contents or undo remote effects. Use native rewind/branch operations after quiescence, not arbitrary edits to harness files. No public branch entity is needed initially.

Avoid whole-transcript writes each turn. Also measure native checkpoint/upload growth: copying a growing native store after every turn recreates quadratic cumulative writes even if Brain's event log is append-only. Use native increments when available or less frequent snapshots with an explicit older recovery boundary. No generic content-addressed layer is required.

## Release policy

Clean breaks are the default for all Brain code, APIs, protocols, and storage formats on every release. No backward compatibility or versioning scheme is implied by launch maturity. Pinning upstream packages and identifying build artifacts enables reproducibility and same-build recovery; it does not introduce historical readers, schema migrations, or protocol negotiation. Any exception needs an explicit decision. Unreadable state must fail clearly rather than silently reset.
