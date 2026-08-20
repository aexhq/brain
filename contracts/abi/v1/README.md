# Brain ↔ hand ABI v1

The wire contract between the **brain** (LLM harness) and a **hand** (tool executor inside a
microVM). `abi.json` is the normative message schema; this file is the normative semantics. The
sealed tool set for the curated image is `tools/manifest.json`, pinned by `tools/manifest.digest`.
Worked examples of every message live in `../../examples/abi/` and are checked in CI.

Vocabulary: **running** (VM alive), **suspended** (AWS keeps RAM + disk, ~1 s back), **released**
(VM destroyed, workspace files synced to storage, ~3 s back into a fresh VM), **workspace sync**
(files only, incremental, automatic), **generation** (one hand incarnation), **lane** (a persistent
shell environment inside the hand), **operation** (one tool call with a durable identity),
**job** (a detached operation still running after `start` returned).

## 1. Transport

One WebSocket per hand, opened by the brain (on AWS Lambda MicroVM: through the VM's endpoint,
authenticated by AWS's JWE token; on an own fleet later: over vsock). Every message is one JSON
**text** frame. Requests and responses are multiplexed by `Request.id`; the hand may answer out of
order, so a long `poll` never blocks a `cancel`. Frames are bounded by `limits.max_frame_bytes`;
anything larger travels by offset (`poll`) or by URL (`put`/`persist`/`sync`), never fragmented.

| Direction | Frame | Schema type |
| --- | --- | --- |
| brain → hand | `{id, fence, generation_id?, call: {op, args}}` | `Request` |
| hand → brain | `{kind: "response", frame: {id, result}}` | `HandFrame` / `Response` |
| hand → brain | `{kind: "hand_status", frame: {...}}` | `HandFrame` / `HandStatusEvent` |

`result` is `{status: "ok", reply: {op, body}}` or `{status: "error", error: AbiError}`.

**Versioning.** `hello` carries `protocol.major/minor`. Major must match exactly; a hand that
cannot serve the brain's major answers `protocol_unsupported` and closes. Minor is additive and
informational. **Unknown fields are ignored** on both sides — the schema deliberately does not set
`additionalProperties: false`.

**Ownership.** Every request carries the brain's `fence` (monotonic ownership token). A request
whose fence is lower than the highest the hand has accepted is refused with `fence_stale` and has
no effect. Every request except `hello` carries `generation_id`; a mismatch is
`generation_mismatch`, no effect.

**Delivery.** At-least-once. `start` is idempotent within a generation by
`(operation_id, call_hash)`; `release`, `cancel`, `lane_close`, `put`, `persist`, `sync` are safe
to repeat. `poll` is read-only.

**Reconnect.** Losing the WebSocket is not losing the hand (I10). The brain reconnects and sends
`hello` with `expected_generation_id`. If the hand answers with the same `generation_id`, every
non-released operation is listed in `operations` and still queryable; if it answers with a
different one, the previous generation's state is gone and the brain reports every in-flight
call as `interrupted` (never replayed). A hand that cannot be reached at all is declared lost by
the brain's adapter (`hand.lost` on the session API) — the hand never reports its own death.

## 2. Operations

| op | Purpose | Answer |
| --- | --- | --- |
| `hello` | authenticate, seal the tool manifest, restore the workspace on a fresh generation, re-attach on reconnect | limits, paths, tools, live lanes and operations |
| `start` | begin a tool call, attached (`detach: false`) or as a background job (`detach: true`); optionally wait up to `wait_ms` so short calls cost one round trip | `OperationView` + first output slices |
| `poll` | status + output from byte cursors; waits up to `wait_ms` for new bytes or terminal | `OperationView` + slices |
| `cancel` | SIGTERM → `grace_ms` → SIGKILL → terminal `cancelled` | accepted flag + view |
| `release` | brain has committed the results; hand deletes spill files and forgets the operations | released / unknown ids |
| `lane_close` | destroy a lane (not lane 0); cancels its attached operation; does not kill jobs it started | closed + cancelled ids |
| `put` | files in: download from presigned GET URLs (or small inline payloads) into the sync scope | written paths + digests |
| `persist` | files out: upload a path or an operation stream to a presigned PUT URL | sizes + digests |
| `sync` | workspace sync: diff the sync scope against the last manifest, upload one pack + one manifest to presigned PUT URLs | counts + bytes |
| *(event)* `hand_status` | idle signal + live jobs + memory pressure; on every transition and every `heartbeat_ms` | — |

### 2.1 `hello`

* `session_token` is the per-session secret the hand was launched with. Mismatch →
  `unauthorized` and the connection is closed. This is our authentication of the brain to the
  hand, layered under AWS's endpoint token.
* **Tool manifest sealing (I1).** The brain sends the digest it sealed at session create. A hand
  that cannot serve that exact manifest answers `tool_manifest_mismatch` and the session fails —
  a tool set must never drift mid-session (one appended tool definition destroyed a 6,103-token
  prompt cache entry). Only on the very first hello of a session may the brain omit the digest and
  adopt the hand's. A newer hand image must keep serving older manifests it has ever shipped, so a
  session created months ago still resumes; a manifest is versioned by digest, not by image tag.
* `env` is applied to lane 0 and inherited by every lane. It is customer data — never a platform
  credential (I8).
* `restore` is present when the session has synced before and this is a fresh generation. The hand
  fetches the manifest, then the packs it references, extracts every entry, then answers. The
  response's `restore` reports what it did. Restore failure is `restore_failed` (fatal for this
  hello; the brain decides whether to retry or fail the session).
* `hello` is idempotent. Lane 0 exists and is ready before it returns.

### 2.2 `start`

* Validation precedes side effects: `tool_not_found` and `tool_input_invalid` (against the
  manifest `input_schema`) are answered before anything runs.
* **The hand records the operation before it spawns the child**, so an ambiguous dispatch is
  recorded as possibly-run rather than lost.
* Idempotent by `(operation_id, call_hash)`: a replay answers the existing operation with
  `replayed: true` and runs nothing. Same `operation_id` with a different `call_hash` →
  `operation_idempotency_conflict`. `call_hash` = SHA-256 over the RFC 8785 canonical JSON of
  `{tool, input, lane, cwd, detach, bounds}` (absent optionals as `null`);
  `brain_protocol::tools::call_hash` implements it and the examples pin values.
* `lane` names a persistent lane (created on first use, up to `limits.max_lanes`) or an ephemeral
  lane forked from `parent`. An attached operation holds its lane until terminal (`lane_busy`
  otherwise); a detached one does not hold the lane.
* `cwd` is a per-call parameter defaulting to `paths.workspace`. The ABI never mutates a lane's
  cwd; a `cd` inside a shell command is consumed by that command. This matches observed usage,
  where 98.1% of directory changes were part of a command such as `cd x && cmd`.
* `wait_ms` (attached only): the hand waits up to this long for the operation to become terminal
  before answering, capped by `limits.max_poll_wait_ms`; `max_bytes` bounds the stdout/stderr
  slices included from offset 0. Most calls therefore complete in one round trip.
* `bounds` merges over `limits.default_bounds`. `timeout_ms` → `deadline_exceeded`.
* `run_command`-style tools are **opaque** (I3): the hand never parses, allow-lists or routes the
  command string.

### 2.3 Operations, streams, output

Every operation owns two byte streams, `stdout` and `stderr`. Command tools write the child's
output there; typed tools (`read`, `ls`, …) write their human-readable result to `stdout` and
diagnostics to `stderr`, and additionally return a small typed `output` validated against the
manifest's `output_schema` (`tool_output_invalid` → outcome `failed`).

* **Bytes never travel as a tool result (I7).** Output is written to a per-stream spill file under
  `paths.spill_dir` (`spill_path`, readable by the agent's own tools) and read by offset in
  bounded, base64 slices. Byte offsets are the authority: no gaps, no duplicates.
* Retention is `bounds.max_retained_bytes` per stream (tail); `retained_from` says where the
  retained region starts, and a poll below it answers `operation_output_evicted` — absent is not
  zero. On terminal with nothing evicted the hand reports the stream's `sha256`.
* **Never wait on pipe EOF (I6).** The hand waits on the direct child's exit; a grandchild holding
  stdout does not keep the operation alive. Its output continues into the spill file.
* `Outcome`: `completed` (the tool ran to its end — exit code 1 is `completed` with
  `exit_code: 1`, data for the model), `failed` (the hand could not run it, `error` set),
  `cancelled`, `deadline_exceeded`, `interrupted` (unknown; never replayed).
* `usage` and `exit_code` are observations from customer-controlled code, never billing authority
  (I9).

### 2.4 Background jobs

`detach: true` starts the child in its own session (`setsid`) with a per-job output file, answers
immediately, and does not hold the lane. The job is owned by the operation registry, not by the
lane that started it — `lane_close` does not kill it. It stays in `hand_status.live_jobs` until
terminal. There is **no default cap** on job lifetime; the brain enforces the session's optional
`max_background_minutes` by `cancel`. At session end: SIGTERM → grace → SIGKILL.

### 2.5 `poll`, `cancel`, `release`

* `poll` waits up to `wait_ms` for any cursor to gain bytes or the operation to become terminal,
  then answers what exists now (never longer). Polling a terminal operation is legal until release.
* `cancel` on a terminal operation is not an error: `accepted: false`, current view.
* `release` is what bounds hand memory without a timer the brain cannot see. Release only after
  the result is durably committed: after release, `poll` is `operation_not_found` and a replayed
  `start` would run again.

### 2.6 Lanes

A lane is a persistent shell environment (env vars, shell functions, umask). Lane `0` is the root
lane and always exists. The brain owns the lane↔agent mapping; the hand is subagent-unaware. An
**ephemeral** lane inherits its parent's env at fork and discards its own mutations when its
operation ends or it is closed — this is what parallel tool calls in one assistant message use.
The filesystem is shared by all lanes; concurrent writes are last-writer-wins. Environment values
never cross the ABI back to the brain.

### 2.7 Files in and out, workspace sync

The hand holds no cloud credential (I8). Every transfer uses a short-lived, single-object presigned
URL minted by the trusted side:

* `put`: GET URL (+ expected `bytes`, `sha256`; mismatch = `checksum_mismatch`, nothing written) or
  an inline base64 payload up to `limits.max_inline_put_bytes`. Paths must resolve (through
  symlinks) inside the sync scope: `path_outside_scope`.
* `persist`: PUT URL per item; source is a path or an operation stream. Bounded by
  `limits.max_persist_bytes` per item. The reply carries size + digest; the trusted side records
  the artifact.
* `sync`: brain-driven — at turn end, every `sync_interval` mid-turn, before any release or
  platform termination (the 8 h wall), and on explicit request. The hand diffs the sync scope
  (`hello.sync.roots` minus `exclude`) against the last manifest by `(size, mtime_ns)` then content
  hash, writes one **pack** (`tar+zstd`, entries named by absolute path without the leading slash)
  holding added and modified files, and one **manifest** (`SyncManifest`) listing the full tree —
  each file entry pointing at the pack that holds its exact content. Nothing is uploaded when
  nothing changed (`changed: false`). `full: true` packs every file (compaction); the brain asks
  for it when `packs_referenced` grows large. Restore is the inverse: manifest → packs → extract.

### 2.8 The idle signal

`hand_status` is published on every idle↔busy transition, when a job ends, and every
`heartbeat_ms`. `idle_for_ms` is 0 while anything is in `inflight` or `live_jobs`. The brain's
adapter keeps the VM alive (keepalive) while jobs are live so AWS's 180 s idle suspend does not
fire under a running job; when nothing is live it lets the suspend happen. `pressure` is advisory;
the guest may be terminated for out-of-memory without advance notice.

## 3. Errors

`AbiError{code, message, retryable, details}`. `retryable: true` only when repeating the identical
request later is safe and may succeed (`resource_exhausted`, `transfer_failed`); never for opaque
work. Codes: see `ErrorCode` in the schema.

## 4. Invariants (conformance)

| # | Invariant |
| --- | --- |
| I1 | The tool set is sealed at `hello` by digest for the session's life |
| I3 | Command strings are opaque; never parsed, allow-listed or routed |
| I6 | The hand waits on the direct child's exit, never on pipe EOF |
| I7 | File bytes never cross the ABI as tool results; output is spilled and read by bounded slices |
| I8 | The hand holds no platform credential; transfers use presigned single-object URLs |
| I9 | Exit status, output and usage are customer-controlled observations, never billing authority |
| I10 | Connection loss ≠ hand loss; the generation id says whether state survived; nothing is replayed |

## 5. MVP simplifications

Dropped: `signal`, `lane_info`, `probe`, streaming `update`, `terminal` push, effect classes,
credit-based backpressure, hard version match, reject-unknown-fields.
Folded: `result` into `poll` (one status+output view for start/poll/cancel).
Added: `sync` for durable workspace state, `wait_ms` on `start`/`poll`
(one round trip for short calls, no push channel needed), presigned-URL transfers, the pack +
manifest format, `session_token` in `hello`.
