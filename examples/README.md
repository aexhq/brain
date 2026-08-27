# Brain examples

These examples use the public TypeScript SDK and HTTP API against a locally running Brain server.

| Example | What it shows |
| --- | --- |
| `basic-session.mjs` | Create a session, run one model turn, and inspect its events. |
| `event-history.mjs` | Read the committed journal and resume from an event cursor. |
| `session-lifecycle.mjs` | List, reopen, cancel, end, and delete a session. |
| `raw-http.mjs` | Admit an Agentloop and run a session using only the HTTP contract. |

On Linux, from the repository root, install dependencies and build the two Brain executables:

```sh
npm ci
cargo build --release -p brain-server --bin brain -p brain-loophost --bin brain-loop-worker
```

Start Brain in one terminal:

```sh
BRAIN_DATA_DIR="$PWD/brain-data" \
BRAIN_LOOP_WORKER="$PWD/target/release/brain-loop-worker" \
./target/release/brain --listen 127.0.0.1:8080
```

In another terminal, provide a Vercel AI Gateway key and run any example:

```sh
export VERCEL_AI_GATEWAY_API_KEY="..."
npm run example:basic
npm run example:events
npm run example:lifecycle
npm run example:http
```

Set `BRAIN_BASE_URL`, `BRAIN_API_TOKEN`, or `BRAIN_MODEL` to override their defaults. The examples
default to `http://127.0.0.1:8080` and `openai/gpt-5-mini`.

`session.events(cursor)` reads committed journal events from that cursor. It is not an external
queue or an at-least-once delivery guarantee. Applications that forward events own their queue,
cursor persistence, retries, and deduplication.
