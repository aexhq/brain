# SDK user journeys

These tests import the public `@aexhq/brain` package and exercise a real Brain server, Wasmtime
worker, and local journal. The model speaks scripted OpenAI SSE; Environment providers run over
HTTP. No model account, cloud deployment, SDK transport stub, or internal SDK import is needed.

`npm run test:journeys` runs four files concurrently using Node's built-in test runner. Each file
owns its server, worker process group, random ports, credentials, and temporary data directory.
Tests within a file run in order. CI builds the binaries and Components once in the existing
Linux worker job, then runs all journeys as a required part of `build-test`.

| Public functionality | Journey coverage |
| --- | --- |
| `Brain` / `BrainClient`, `withToken`, custom `fetch`, `request`, `BrainError` | Authenticated and unauthenticated clients, isolated derived credentials, real request tracing, missing sessions, invalid options |
| `component`, `agentloop`, `admit`, `admitAgentloop`, `admitTool`, inspection | File/bytes/HTTP artifacts, identity reuse, parallel preparation, rejected artifacts, prepared session creation, native Tool execution |
| `sessions.create/get/list`, initial transcript, system, response format, idle policy | Full conversation lifecycle, seeded context, reopen through another client, idempotent creation with and without resident Tools, conflicting keys, parallel conversations |
| `send`, `state`, `id`, `transcript` | String/structured input, multiple suspended turns, idempotent sends, invalid input, cold reads, committed input after interruption |
| `cancel`, `end`, `delete` | Running and idle cancellation, model and resident Tool cancellation, repeated keyed operations, ended-session reads, invalid deletion, independent sessions |
| `events`, `stream`, client `stream` | Durable cursors, a full page boundary, replay-to-live delivery, reconnect, abort, authentication, session isolation |
| `tool` resident handlers, options, schemas, context | Progress ordering, input/output errors, handler errors, deadlines/signals, concurrent sessions, protected Event rejection |
| `residentHost`, `residentHostCredentials`, reattachment | Save credentials, close the host connection, reject mismatched bindings, restore matching handlers, preserve active-call cancellation on creation replay |
| `environment`, `brainWasm`, placed Tools and inspection | Independent authenticated providers, option/binding configuration, lazy allocation, expiry without retry, native workspace persistence/isolation, missing grants |
| `timeoutMs` | Explicit client timeout leaves the server's execution observable and does not retry the model call |

The pagination case emits fewer than the permitted Events per turn across enough turns to cross
the real page boundary. It is a correctness check, not a throughput or memory benchmark. Cancellation
tests synchronize with actual model/Tool entry rather than guessing execution progress with sleeps.

## Run locally on Linux

After `npm ci && npm run build`:

```sh
cargo build -p brain-server --bin brain -p brain-loophost --bin brain-loop-worker
cargo build --manifest-path tests/fixtures/diagnostic-agentloop/Cargo.toml --target wasm32-wasip2 --release
cargo build --manifest-path tests/fixtures/diagnostic-tool/Cargo.toml --target wasm32-wasip2 --release
cargo build --manifest-path examples/reference-agentloop/Cargo.toml --target wasm32-wasip2 --release
export BRAIN_TEST_SERVER="$PWD/target/debug/brain"
export BRAIN_TEST_WORKER="$PWD/target/debug/brain-loop-worker"
export BRAIN_TEST_AGENTLOOP_PACKAGE="$PWD/tests/fixtures/diagnostic-agentloop/target/wasm32-wasip2/release/diagnostic_agentloop.wasm"
export BRAIN_TEST_TOOL_COMPONENT="$PWD/tests/fixtures/diagnostic-tool/target/wasm32-wasip2/release/diagnostic_tool.wasm"
export BRAIN_TEST_REFERENCE_AGENTLOOP="$PWD/examples/reference-agentloop/target/wasm32-wasip2/release/reference_agentloop.wasm"
npm run test:journeys
```

Missing binaries or Components fail the suite; no journeys are conditionally skipped. Use WSL for
these Linux process/worker tests on Windows. Portable SDK unit tests still run with `npm test`.
