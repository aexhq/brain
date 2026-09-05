# ADR-040: Identify records by session and sequence, and everything inside a session by name

- Decision date: 2026-09-05
- Status: Proposed
- Compiled: 2026-09-05

Extends: [ADR-020: Name effects by session and sequence, with one lifecycle vocabulary](2026-09-02-03-record-identity.md); [ADR-024: Use one SessionConfig and keep hashes local to their actual purpose](2026-09-04-02-simple-config.md).

## Context

ADR-020 made `(session_id, sequence)` the name of every record and effect. Several identifiers that predate it, or that served the shared-Environment design ADR-033 removed, are still minted, stored, validated, and copied into public Events, the Agentloop's per-turn feed, and telemetry. Each one is either computed from fields beside it or names something a session-scoped name already names.

| Identifier | Minted by | What it is today | What already names the same thing |
| --- | --- | --- | --- |
| `event_id` | Brain | `evt_{session_id}_{sequence}`, on every Event and telemetry record | `(session_id, sequence)`; SSE already uses `sequence` as its id |
| `attachment_id` | Brain, random `att_…` | Minted once at attach; attach happens once per Environment, at create | `(session_id, environment)`, since an Environment belongs to one session |
| `directory_generation` | Brain | Always `1`, carried inside `EnvironmentBinding` beside a second copy of `environment_id` | `environment_id` |
| `binding_id` | Brain | `model_{session_id}`, a server-private credential key written into `SessionConfig` | `session_id` |
| `environment_id` | Caller | Must be unique across the whole server, so the SDK derives `env_{sha256(create key)}_{n}` | A caller-chosen name unique within the session, which create already validates |
| `call_id` | Agentloop | The model's `tool_use` id, echoed in `ToolResult` and also forwarded to Environments, hosts, and Components beside `sequence` | `sequence`, on every wire |
| `host_id` | Brain, random `host_…` | A credentialed, cross-session identity for a resident host | Nothing; a host registers before any session exists and serves several |
| `identity` | Loop host, SHA-256 of admitted bytes | The content address of an admitted Agentloop or Tool artifact, named as if a registry assigned it | Nothing; it is the artifact's id, and the name should say so |

`attachment_id` and `directory_generation` were meaningful under ADR-028, when one shared Environment held attachments from several sessions and lived in a server-wide directory. ADR-033 made an Environment declaration belong to one session; the identifiers stayed. Server-wide `environment_id` uniqueness is the same leftover: the resources table and `environments/{environment_id}.json` are keyed by the id alone, which is why the SDK must hash the create key into every Environment name and why the docs have to explain that derivation. The HTTP route already addresses an Environment as `/sessions/{session_id}/environments/{environment_id}`.

The most visible cost is `tool_call_started`. It serialises the whole `ToolBinding` on every call: `environment_id` twice, `attachment_id`, `host_id`, `needs`, `binding_names`, `hosting`, and the opaque `implementation` descriptor. That record is public, is handed to the Agentloop in `TurnInput.events` up to 1,000 per turn inside a bounded input frame, and is copied verbatim into telemetry. The same binding is already recorded once, at `session_creation_ended`, as the admitted configuration.

Tool names show the rule already in use for one kind of thing: a Tool is named by the caller, unique within its session, and referenced by name everywhere. There is no `tool_id`.

## Decision

Brain mints two identifiers for a session: `session_id` at create and `sequence` at every append. Everything a caller declares inside a session, Environments, Tools, and resource names, is a name: caller-chosen, matching the identifier pattern, unique within that session, and referenced as `(session_id, name)`. `host_id` remains the one server-minted identity outside a session; it appears in host registration and in the configuration of the Environment that names that host ([ADR-041](2026-09-05-10-one-execution-model.md)), never on a Tool or in an effect record.

1. Remove `event_id`. A record is `(session_id, sequence)` on every surface: the Event page, SSE, the SDK, the Agentloop feed, and telemetry. A telemetry record that carries a journal record carries its `sequence`.
2. Remove `attachment_id`, `directory_generation`, and `EnvironmentBinding`. An Environment operation is named by `(session_id, environment, sequence)` and carries nothing else about identity.
3. Remove `binding_id`. The server keys a session's model credential by `session_id`.
4. Scope Environment names to the session and call the field `environment`, as Tools use `name`. The caller names every Environment; the name is a required part of the declaration, and the SDK derives nothing from the create key. The server stores Environment rows under the session's directory.
5. Public records carry references, not copies. `tool_call_started` carries the Tool name, the invocation, and the deadline; where the call goes is the configuration's answer for that name. `tool_call_ended` carries the started sequence and the result without repeating the call id. `environment_*` records name the Environment and carry the request. The full binding lives once, in the configuration recorded at creation.
6. `call_id` is Agentloop content. Brain journals it in the started record and echoes it in the result, but does not forward it on the Environment, host, or Component wires, where `sequence` names the call. A Tool that wants an identifier for its own logging uses `(session_id, sequence)`, which is unique across every record and every telemetry batch.
7. The creation record keeps the full admitted configuration, implementation descriptors included. Recovery rebuilds the session row from that record, and it is the one place a binding lives once effect records stop copying it.
8. An admitted Agentloop or Tool artifact is identified by the SHA-256 of its bytes, and the field is `id`, not `identity`. The `Identity` type, the `agentloop.identity` field, the `brain_component.identity` field, and the admit route parameter are renamed. The value is unchanged: a caller with the same bytes computes the same id.

## Alternatives considered

Filtering identifiers out of projections only would leave them minted, stored, validated, and documented, leave the SDK hashing Environment names, and define the Event shape in a second place; ADR-032 makes projections views of the journal, not a second schema. Keeping `event_id` as a convenience string keeps two names for one record, which ADR-020 rejected for effects. Keeping `attachment_id` for a future re-attach anticipates a feature ADR-037 does not promise; a re-attach, if it ever exists, is a journaled operation with its own sequence. Prefixing names with the session id on the wire is `event_id` again: a derivation carried as data.

## Consequences

This is a clean-break contract change under ADR-030 and ADR-031. `Event`, `EnvironmentCommand`, `EnvironmentOperation`, `BoundTool`, `AgentloopRef`, the Environment call route, the Component `invocation` record, the SDK's `SessionEvent`, and the Environment row layout in the data directory change together with the generated contracts and docs. Consumers correlate by `(session_id, sequence)`; a log line or sink that wants one string composes it. Records shrink to a name, a target, and a payload, and the Agentloop feed and telemetry shrink with them. An idempotent create retry sends the same body without hashing, because the body contains only names. The SDK's Environment factories gain a required name; two Environments with one name in a session are refused at create, as today. Environment drivers correlate by `(session_id, environment, sequence)`, which the authoring guide already instructs.

Not affected: `host_id` at the host boundary, `tool_use` ids inside messages, which are provider content, artifact ids and idempotency keys under ADR-024 and ADR-036, and resource names. What an Environment is, and the removal of binding names and values, are decided in [ADR-041](2026-09-05-10-one-execution-model.md).

## Sources

- Current reference: [crates/brain-protocol/src/ids.rs](../../crates/brain-protocol/src/ids.rs), the minted shapes.
- Current reference: [crates/brain/src/journal/record.rs](../../crates/brain/src/journal/record.rs), the `event_id` derivation.
- Current reference: [crates/brain-server/src/service.rs](../../crates/brain-server/src/service.rs), the `binding_id` derivation.
- Current reference: [crates/brain-server/src/environment/registry.rs](../../crates/brain-server/src/environment/registry.rs), `attachment_id` minting and the constant `directory_generation`.
- Current reference: [crates/brain-server/src/environment/resources.rs](../../crates/brain-server/src/environment/resources.rs), server-wide Environment rows.
- Current reference: [crates/brain/src/session/actor.rs](../../crates/brain/src/session/actor.rs), the `tool_call_started` payload and the per-turn feed.
- Current reference: [packages/brain-sdk/src/client.ts](../../packages/brain-sdk/src/client.ts), Environment name derivation from the create key.
- Current reference: [contracts/environment/v1/examples/invoke.json](../../contracts/environment/v1/examples/invoke.json).
- Current reference: [crates/brain/src/journal/session_store.rs](../../crates/brain/src/journal/session_store.rs), recovery folding the configuration out of `session_creation_ended`.
- Brain implementation/history: [79602a5](https://github.com/aexhq/brain/commit/79602a5) (`event_id`), [2180ed1](https://github.com/aexhq/brain/commit/2180ed1) and [820a37c](https://github.com/aexhq/brain/commit/820a37c) (`attachment_id`), [dc0ac55](https://github.com/aexhq/brain/commit/dc0ac55) (`directory_generation`, Environments as server resources), [6081ebb](https://github.com/aexhq/brain/commit/6081ebb) (`binding_id`), [a21ae6e](https://github.com/aexhq/brain/commit/a21ae6e) (`host_id`).
- [Claude session `session_01LdNUcSZgJNte7vx1ShXyt7`](https://claude.ai/code/session_01LdNUcSZgJNte7vx1ShXyt7), 2026-09-05: User asks to stop polluting Events and telemetry with internal identifiers, keeping `session_id` and deriving the rest from it plus unique naming within a session. Later in the session the user removes `call_id` from Tool wires, requires caller-given Environment names, keeps the full configuration in the creation record, and renames the artifact content address to `id`.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
