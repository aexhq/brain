# Platform examples

These are starting configurations for one Brain owner. Templates are locally checked; they are not evidence of a live deployment on every platform. Keep API keys in environment variables or the platform's secret store, and pin deployed images by digest from a passing CI run.

## Docker Compose

Set `BRAIN_API_KEY`, `OPENAI_API_KEY`, and `TOOL_API_KEY`, then run from the repository root:

```sh
docker compose -f examples/platforms/docker/compose.json up --build
```

Brain listens on localhost:8080 and reaches the resource tool service over a private Compose network. Enable `web_fetch` in a new session. The named volume retains the local cache; `docker compose down` preserves it unless you explicitly remove volumes. A mounted volume is not an external backup.

## Fly.io

Edit `fly/brain.json` for your app URL and archive bucket. From `examples/platforms/fly`, create your app, configure `BRAIN_API_KEY`, `OPENAI_API_KEY`, `ARCHIVE_ACCESS_KEY`, and `ARCHIVE_SECRET_KEY` with Fly secrets, then deploy:

```sh
fly deploy --config fly.toml --ha=false
fly scale count 1
```

The example uses an ephemeral cache and external checkpoints, disables idle auto-stop, and allows 30 seconds for shutdown. `immediate` deployment replaces the single owner instead of overlapping instances. Before a planned replacement: stop admission, settle/interrupt, flush, stop the old owner, then restore and explicitly resume on the new owner. A volume is optional and local to one Machine, not replication. Multiple instances require an application placement layer; Fly replay can route to a chosen owner but does not provide ownership fencing.

## AWS ECS/Fargate

Edit `ecs/brain.json`, build its small configuration image using a verified Brain digest, and push it to your registry. Replace account, image digest, role, and secret placeholders in `ecs/task-definition.json`. The execution role pulls the image and reads the two referenced secrets; the task role needs S3 read/write under only the checkpoint prefix. Pre-create the log group.

Use one task, a private network, and an HTTPS load balancer. Set service deployment `minimumHealthyPercent: 0`, `maximumPercent: 100`, and desired count 1 to avoid overlapping writers; perform the same explicit handoff before replacing the task. Fargate task storage is the disposable local cache. Add the PostgreSQL index if the application needs cold-session discovery. Load-balancer stickiness alone does not make multi-writer execution safe.

## Cloudflare

`cloudflare/worker.mjs` is a complete R2 **tool** Worker with read/write operations, body limits, authenticated Brain identity, and tenant prefixes. Edit the R2 binding, set `TOOL_API_KEY` with Wrangler secrets, and deploy using the official Wrangler CLI. Leave `ALLOW_WRITE=false` for read-only operation. Bind ordinary HTTP tools to its `/read` and `/write` endpoints with `{key}` and `{key,text}` argument schemas. Never use the checkpoint bucket for model-accessible objects.

The Brain binary and Pi child process need a container or VM; a normal Worker cannot run them. Cloudflare Containers can host that runtime, with Durable Objects as a platform routing option. That runtime recipe remains experimental until container lifecycle and handoff tests pass.

## Application backends

Keep Brain credentials on your backend. A Next.js route, Python service, or Worker should authenticate its own end user, resolve that user's permitted session IDs, and proxy only allowed operations. Use the TypeScript/Python SDKs for create, send, stream, interrupt, flush, and resume; never forward a manager token to the browser. `examples/session.mjs` is the runnable TypeScript lifecycle example; `examples/session.py` is its Python counterpart.

Cloud Run request-based CPU is unsuitable for a turn that continues after HTTP 202. Instance-based CPU addresses that specific issue, but termination windows, request limits, and owner routing still need qualification. It is not an advertised runtime deployment target yet.

Sources: [Fly configuration](https://docs.fly.io/reference/configuration), [Fly volumes](https://docs.fly.io/volumes/overview), [ECS task parameters](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/task_definition_parameters.html), [Cloudflare Containers](https://developers.cloudflare.com/containers/).
