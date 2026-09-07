# Brain examples

These examples use the public TypeScript SDK and HTTP API against a locally running Brain server.

| Example | What it shows |
| --- | --- |
| `basic-session.mjs` | Create a session, run one model turn, and inspect its events. |
| `event-history.mjs` | Read committed public Events and resume from a journal cursor. |
| `session-lifecycle.mjs` | List, reopen, cancel, end, and delete a session. |
| `raw-http.mjs` | Admit raw Agentloop Component bytes and run a session using only HTTP. |
| `example-brain.mjs` | Wrap a compiled Agentloop Component in the SDK factory. |
| `reference-agentloop/` | A Rust Agentloop Component written against Brain's public contracts alone. |
| `lazy-environment.mjs` | A standalone Environment: logical setup with needs, lazy allocation, idle expiry, explicit restart. |
| `loop-environment.mjs` | A standalone Environment that runs an Agentloop on another server, calling Brain's turn routes back. |

On Linux, from the repository root, install dependencies, build the SDK, and build the two Brain
executables:

```sh
npm ci
npm run build
cargo build --release -p brain-server --bin brain -p brain-env-worker --bin brain-env-worker
```

Start Brain in one terminal:

```sh
BRAIN_DATA_DIR="$PWD/brain-data" \
BRAIN_ENV_WORKER="$PWD/target/release/brain-env-worker" \
./target/release/brain --listen 127.0.0.1:8080
```

An Agentloop is a WebAssembly Component implementing `crates/brain-env/wit/agentloop/agentloop.wit`. Brain
accepts the compiled Component as raw Wasm; it does not build extension source. Compile one with its
own language toolchain, or build the reference loop in this directory:

```sh
cargo build --manifest-path examples/reference-agentloop/Cargo.toml --target wasm32-wasip2 --release
```

In another terminal, provide that file and a Vercel AI Gateway key, then run any example:

```sh
export VERCEL_AI_GATEWAY_API_KEY="..."
export BRAIN_AGENTLOOP_WASM="$PWD/examples/reference-agentloop/target/wasm32-wasip2/release/reference_agentloop.wasm"
npm run example:basic
npm run example:events
npm run example:lifecycle
npm run example:http
```

Set `BRAIN_BASE_URL`, `BRAIN_API_TOKEN`, or `BRAIN_MODEL` to override their defaults. The examples
default to `http://127.0.0.1:8080` and `openai/gpt-5-mini`.

`lazy-environment.mjs` needs no Brain server. It serves the Environment protocol at
`POST /v1/operations` on `127.0.0.1:8090`, and checks `REFERENCE_ENV_TOKEN` as a bearer token when
that is set:

```sh
node examples/lazy-environment.mjs
```

A session reaches it by naming its address: an `environment({ url, credential })` factory in the
SDK, or an entry `{ "name": "echo", "driver": "http", "url": "http://127.0.0.1:8090" }` in a raw
create request. `npm test -w examples` runs its unit test for concurrent allocation, expiry, and
explicit restart.

`loop-environment.mjs` runs an Agentloop outside Brain's process on `127.0.0.1:8091`. Place a
session's Agentloop in it the same way, and Brain sends each turn with the address of its turn
routes and the token that opens them; the loop calls the model, dispatches Tools, and emits Events
through those routes, and Brain journals every call. Brain must be reachable from the loop's
process: set `BRAIN_PUBLIC_URL` when that is not the listen address.

`session.events(cursor)` reads the public Event projection from the canonical journal. It is not an external
queue or an at-least-once delivery guarantee. Applications that forward events own their queue,
cursor persistence, retries, and deduplication.
