# Brain examples

Start Brain with a harness, then run an application example. Node 22.19+ is required;
OpenCode uses Bun 1.3.14. The server image includes the pinned upstream adapters.

```sh
docker run --rm -p 127.0.0.1:8080:8080 -e BRAIN_API_TOKEN=example \
  -e 'BRAIN_AGENTLOOPS={"pi":{"command":"brain-agentloop-pi"}}' \
  -v brain-data:/var/lib/brain ghcr.io/aexhq/brain-agentloops:latest
export BRAIN_API_TOKEN=example
export VERCEL_AI_GATEWAY_API_KEY=your-key
npm ci
npm run build
npm run example:basic
```

`basic-session.mjs`, `event-history.mjs`, `streaming-session.mjs`,
`submitted-turn.mjs`, and `session-lifecycle.mjs` cover application operations.
`raw-http.mjs` uses the same API without the SDK. `order.ts` adds a function in the
application and uses `OPENAI_API_KEY`.

`harness.mjs` is a small custom startup implementation. Run it with
`BRAIN_AGENTLOOPS='{"custom":{"command":"node","args":["/absolute/path/examples/harness.mjs"]}}'`.
Select `agentloop: { name: "custom" }` in the application. Applications select installed names;
they cannot upload harness code.

`lazy-environment.mjs` and the Python Environment examples demonstrate tool runtimes.
Their preparation and integration checks are described in [CONTRIBUTING.md](../CONTRIBUTING.md).
