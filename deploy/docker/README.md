# Run the distributed services

Install Docker with Compose and Node.js 24+. From the repository root:

```sh
node scripts/docker-env.mjs
docker compose -f deploy/docker/compose.yaml --profile images build
docker compose -f deploy/docker/compose.yaml up -d --wait
```

The environment script creates `deploy/docker/.env` with random API, service and encryption keys. It refuses to overwrite an existing file. Keep this ignored file with the volumes: losing the encryption key prevents recovery of stored credentials.

Brain listens on `http://127.0.0.1:8080`. Storage, Tools, Sandbox and the OpenTelemetry Collector have no published host ports. The Pi and tool-runtime build targets supply execution images; Compose does not keep those two profiles running. Sandbox starts their containers on demand.

## Run a session

Build the client with `npm --prefix sdks/typescript ci` and `npm --prefix sdks/typescript run build`. Set `OPENAI_API_KEY` in your terminal, then use the CLI without copying the Brain key into commands:

```sh
node --env-file=deploy/docker/.env sdks/typescript/dist/cli.mjs create --id support/42 --model openai/gpt-4.1-mini --credential-env OPENAI_API_KEY
node --env-file=deploy/docker/.env sdks/typescript/dist/cli.mjs send support/42 "Say hello."
node --env-file=deploy/docker/.env sdks/typescript/dist/cli.mjs observe support/42
```

Stop observation with Ctrl+C; the agent turn continues. `history`, `get`, `interrupt`, `resume`, `flush`, `sync`, `close` and `delete` accept the same session ID. The TypeScript SDK uses the same API. A session ID is supplied by your application; the SDK generates one when omitted.

## Configure the services

Edit `brain.json`, `storage.json`, `tools.json` and `sandbox.json`, then run `docker compose -f deploy/docker/compose.yaml up -d --force-recreate --wait`.

- Brain: public tokens, model provider endpoints, admission limits, dependency URLs and externally assigned owner/epoch.
- Storage: local cache budgets/TTLs, object backend and metadata backend.
- Tools: remote bindings, optional `providers_file`, public executor authentication and encrypted tool credentials.
- Sandbox: approved images, network/resource profiles, environment capacity and its controller identity.

Use the same credential keyring for Brain and Tools. Pass backend secret variables to the Storage service through Compose `environment`; naming a variable in JSON does not forward it into a container. Mount a `providers_file` in Tools when installing integrations from `tools/integrations`.

## Persistence and recovery

`brain-work` is disposable materialized state. `storage-cache` holds locally acknowledged writes that may not have reached the external backend. `external-storage` holds the example filesystem objects and SQLite metadata. All three are Docker volumes on this host. Restarting containers preserves them; deleting volumes destroys them. The external example is a separate copy, not off-host disaster recovery.

Use `flush` to wait for a settled snapshot, its transcript and attachments to be committed externally; use `sync` to inspect the actual published boundary. Replace the example external backends with S3/R2/DynamoDB/PostgreSQL/Supabase for off-host storage. A Storage partition must have exactly one local-volume writer. The platform owns partition routing and ownership transfer.

The initial composition runs Pi. Each active/warm session gets a separate non-root container. Uploaded modules run in a different network-disabled container. The Sandbox controller holds the Docker socket and is part of the trusted deployment; user containers receive neither that socket nor service secrets.

## Observe and stop

```sh
docker compose -f deploy/docker/compose.yaml ps
docker compose -f deploy/docker/compose.yaml logs --tail 100 brain storage tools sandbox
docker compose -f deploy/docker/compose.yaml logs --tail 100 telemetry
docker compose -f deploy/docker/compose.yaml down
```

`down` preserves the volumes. Brain readiness checks its dependencies without model calls. Configure the collector's exporter for your telemetry backend; the supplied collector prints aggregate trace and metric batches. SDK applications configure their own OpenTelemetry providers through `configureTelemetry`.

## Verify this composition

```sh
node scripts/container-smoke.mjs
node scripts/adapter-services.mjs
```

The smoke test temporarily replaces model/tool endpoints with local fixtures, exercises isolated Pi, all tool flavours, attachments, restart/restore, CLI and external flush, then restores the base configuration. The adapter command starts disposable PostgreSQL, S3-compatible and DynamoDB Local containers. These tests are not live-provider qualification.

CI publishes the six service/runtime images plus the existing native tool images under `ghcr.io/aexhq/brain*`, tagged `sha-COMMIT` and `main`, after all required jobs pass. Use immutable commit tags when configuring deployment images and Sandbox profiles together.
