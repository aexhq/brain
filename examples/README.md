# Brain examples

New to Brain? Start with the [order lookup quickstart](https://aex.dev/brain/docs/quickstart).
These runnable examples show individual tasks against a local Brain server.

| Example | What it shows |
| --- | --- |
| [order.ts](order.ts) | The complete quickstart, checked with strict TypeScript. |
| [streaming-session.mjs](streaming-session.mjs) | Follow a turn, detect failure and close its subscription. |
| [submitted-turn.mjs](submitted-turn.mjs) | Submit work, close the client and retrieve its outcome later. |
| `basic-session.mjs` | Create a session, run one model turn, and inspect its events. |
| `event-history.mjs` | Read saved progress and catch up after reconnecting. |
| `session-lifecycle.mjs` | List, reopen, interrupt, end and delete a session. |
| `raw-http.mjs` | Run a session through HTTP without the SDK. |
| `example-brain.mjs` | Wrap a compiled Agentloop Component in the SDK factory. |
| `reference-agentloop/` | A Rust Agentloop Component written against Brain's public contracts alone. |
| `lazy-environment.mjs` | Run tools in a service with a shared workspace. |
| `loop-environment.mjs` | Run an agent loop in a separate JavaScript service. |
| [packaged-tools](packaged-tools) | Build a tool package and compile a fresh consumer. |
| [tool-model.mjs](tool-model.mjs) | Let a tool make its own model request. |
| [tool-outcomes.mjs](tool-outcomes.mjs) | Return structured errors alongside a successful output schema. |
| [python-environment.mjs](python-environment.mjs) | Prepare and run a locked Python project in your own environment. |

## Application examples

Use the [quickstart's setup](https://aex.dev/brain/docs/quickstart) for `order.ts`,
`streaming-session.mjs` and `submitted-turn.mjs`. They use OpenAI, Pi and the local `quickstart`
token. Copy the desired file into that app and run `npx tsx order.ts` or `node <example>.mjs`.
The streaming and submission examples also accept `BRAIN_BASE_URL` and `BRAIN_API_TOKEN`.

## Source-build examples

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
BRAIN_API_TOKEN=quickstart \
BRAIN_ENV_WORKER="$PWD/target/release/brain-env-worker" \
./target/release/brain --listen 127.0.0.1:8080
```

Build the included Rust loop (requires the `wasm32-wasip2` target):

```sh
rustup target add wasm32-wasip2
cargo build --manifest-path examples/reference-agentloop/Cargo.toml --target wasm32-wasip2 --release
```

In another terminal, provide that file, the server token and a Vercel AI Gateway key for the
basic, history, lifecycle and raw HTTP examples:

```sh
export VERCEL_AI_GATEWAY_API_KEY="..."
export BRAIN_API_TOKEN=quickstart
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
create request. `npm test -w examples` checks concurrent allocation and caller-controlled teardown.

`loop-environment.mjs` runs an Agentloop outside Brain's process on `127.0.0.1:8091`. Place a
session's Agentloop in it the same way, and Brain sends each turn with the address of its turn
routes and the token that opens them. This minimal loop calls the model once, saves state and
emits events; it does not dispatch tools. Brain must be reachable from the loop's
process: set `BRAIN_PUBLIC_URL` when that is not the listen address.

`session.events(cursor)` reads the public Event projection from the canonical journal. It is not an external
queue or an at-least-once delivery guarantee. Applications that forward events own their queue,
cursor persistence, retries, and deduplication.

### Python project preparation

`python-environment.mjs` exports an Environment handler for operator-selected projects. Supply
`{ projects: { name: { directory, module, setup? } }, uv? }`, then serve its `handle` function
over the Environment protocol in an isolated OS runtime. An implementation descriptor is
`{ type: "python_project", name }`; session setup accepts an empty configuration. Python and
`uv` are Environment bootstrap prerequisites, not dependencies resolved by Brain.

The loader runs `uv sync --locked`, optionally runs a setup module, then invokes the module with
JSON stdin and reads JSON stdout. Preparation is shared by installation directory across
concurrent invocations and sessions. Setup failure prevents execution and remains an explicit
failure until the operator replaces the loader. Bindings can detach without deleting the installation.
No custom setup hook is required for ordinary packaged dependencies.

Run the real preparation tests with `BRAIN_TEST_UV=/path/to/uv node --test examples/python-environment.test.mjs`
from the repository root. CI runs this gate with uv 0.8.15. The locked fixture imports an installed
package, proves setup-before-entrypoint, rejects incompatible placement, and checks concurrency
and failure without mocked Python execution. This example does not itself sandbox processes,
filter network access, or provide durable background jobs.
