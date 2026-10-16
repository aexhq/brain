# Platform examples

`examples/platforms` provides starting deployment configurations for running a single-owner Brain server and its companion tool services across Docker Compose, Fly.io, AWS ECS/Fargate, and Cloudflare.

> **Note:** These templates are validated locally by `tests/platforms.test.mjs`; they are not evidence of a live deployment on every platform. Keep API keys in environment variables or your platform's secret store, and pin deployed container images by digest from a passing CI run.

## Summary of platform templates

| Platform | Path | Role | Ownership & durability model |
| --- | --- | --- | --- |
| **Docker Compose** | `examples/platforms/docker/compose.json` | Brain server + `resources` tool service on a private Compose network | Named local volume (`brain-data`) for local cache; add `persistence` for external backups. |
| **Fly.io** | `examples/platforms/fly/fly.toml`, `fly/brain.json` | Single Fly Machine (`--ha=false`, count `1`, `immediate` strategy) | Ephemeral local cache backed by S3/R2 checkpoints; explicit flush/restore handoff on replacement. |
| **AWS ECS/Fargate** | `examples/platforms/ecs/task-definition.json`, `ecs/brain.json` | Single Fargate task (`desiredCount: 1`, `minimumHealthyPercent: 0`, `maximumPercent: 100`) | Disposable task storage backed by S3 checkpoints and optional PostgreSQL discovery index. |
| **Cloudflare Worker** | `examples/platforms/cloudflare/worker.mjs`, `wrangler.json` | Tenant-scoped R2 HTTP tool service (`/read`, `/write`) | Stateless tool endpoint for Brain; Brain itself still runs in a container or VM. |

## Docker Compose

Set `BRAIN_API_KEY`, `OPENAI_API_KEY`, and `TOOL_API_KEY` (each at least 24 bytes for Brain/tool tokens), then start the stack from the repository root:

```sh
docker compose -f examples/platforms/docker/compose.json up --build
```

- Brain listens on `http://localhost:8080` and calls the `resources` tool service over the internal Compose network (`http://resources:8092`).
- Enable `web_fetch` in `config.tools` when creating a session.
- The named volume (`brain-data`, mounted as Brain's `data_dir` at `/data`) survives `docker compose down` unless you pass `-v`. A mounted local volume is not an external backup—configure `persistence` for off-host durability.

## Fly.io

Edit `examples/platforms/fly/brain.json` for your public app URL and checkpoint archive bucket. From `examples/platforms/fly`, create your Fly app, set `BRAIN_API_KEY`, `OPENAI_API_KEY`, `ARCHIVE_ACCESS_KEY`, and `ARCHIVE_SECRET_KEY` via `fly secrets set`, and deploy a single Machine:

```sh
fly deploy --config fly.toml --ha=false
fly scale count 1
```

- **Single-owner settings:** The configuration uses an ephemeral local cache plus external S3/R2 checkpoints, disables idle auto-stop (`auto_stop_machines = "off"`), allows a 30-second `kill_timeout`, and uses `strategy = "immediate"` so deployments replace the single owner instead of overlapping two writers.
- **Planned replacement:** Before replacing the Machine, stop admission, let active turns settle (or interrupt them), call `POST /sessions/{id}/flush`, stop the old owner, then `POST /sessions/{id}/restore` and `POST /sessions/{id}/resume` on the new owner.
- **Volumes and routing:** A Fly volume is optional and local to one Machine, not cross-host replication. Running multiple Machines requires an application placement layer; `fly-replay` can route HTTP requests to a chosen Machine but does not fence concurrent ownership.

## AWS ECS/Fargate

1. Edit `examples/platforms/ecs/brain.json`, build a configuration image on top of a verified Brain image digest, and push it to your container registry.
2. Replace the account ID, image digest, IAM role ARN, and Secrets Manager ARN placeholders in `examples/platforms/ecs/task-definition.json`, and pre-create the `/brain` CloudWatch log group.
3. Grant the **execution role** permission to pull the image and read `BRAIN_API_KEY` and `OPENAI_API_KEY` from Secrets Manager; grant the **task role** S3 read/write (`GetObject`, `PutObject`) under only the configured checkpoint prefix.
4. Run a single task behind a private network and HTTPS load balancer with `desiredCount: 1`, `minimumHealthyPercent: 0`, and `maximumPercent: 100` to prevent overlapping writers, and follow the same explicit flush → stop → restore → resume handoff when replacing tasks.

> **Note:** Fargate task storage is a disposable local cache. Add `postgres_env` if your application needs cold-session discovery across task replacements. Load-balancer stickiness alone does not make multi-writer execution safe.

## Cloudflare

`examples/platforms/cloudflare/worker.mjs` is a complete Cloudflare Worker implementing a tenant-scoped R2 **tool** service with `/read` and `/write` endpoints, request size bounds, bearer token authentication (`TOOL_API_KEY`), and automatic per-tenant key prefixes (`tools/<base64url tenant_id>/`).

1. Edit the R2 bucket binding in `examples/platforms/cloudflare/wrangler.json`, set `TOOL_API_KEY` with `wrangler secret put TOOL_API_KEY`, and deploy with Wrangler. Leave `ALLOW_WRITE` set to `"false"` for read-only access.
2. Bind HTTP tools in Brain's `tools` config pointing to the Worker's `/read` (`{ "key": "..." }`) and `/write` (`{ "key": "...", "text": "..." }`) URLs. Never point model-accessible tools at Brain's checkpoint bucket.

> **Note:** The Brain Go binary and Pi Node.js child process require a container or VM; a standard Cloudflare Worker cannot host them. Cloudflare Containers can run container images alongside Durable Objects for routing, but that runtime recipe remains experimental until container lifecycle and handoff tests pass.

## Application backends

Keep Brain management credentials on your backend server:

- A Next.js route handler, Python backend, or API gateway should authenticate its own end user, verify which session IDs that user owns, and proxy only permitted operations to Brain.
- Use the TypeScript or Python SDKs (`examples/session.mjs` and `examples/session.py`) on your backend for `create`, `send`, `observe`, `interrupt`, `flush`, and `resume`; never expose a `manage: true` token directly to browser clients.
- **Cloud Run note:** Cloud Run request-based CPU throttling is unsuitable for session turns that continue executing after `POST /sessions/{id}/events` returns `202 Accepted`. Instance-based CPU billing avoids mid-turn throttling, but termination windows, request timeouts, and single-owner routing still require qualification before Cloud Run is supported as a runtime target.

Sources: [Fly configuration](https://docs.fly.io/reference/configuration), [Fly volumes](https://docs.fly.io/volumes/overview), [ECS task parameters](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/task_definition_parameters.html), [Cloudflare Containers](https://developers.cloudflare.com/containers/).

## Next steps

- [Operations & observability](https://aexhq.github.io/brain/docs/guides/operations)
- [Persistence & recovery](https://aexhq.github.io/brain/docs/guides/persistence)
- [Authentication & tenants](https://aexhq.github.io/brain/docs/guides/authentication)
- [Host configuration reference](https://aexhq.github.io/brain/docs/reference/configuration)
