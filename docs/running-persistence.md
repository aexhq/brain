# Persistence and recovery

Brain always keeps active session state on local disk (SQLite for metadata, append-only files for events and native harness state). External persistence is optional: when enabled, a background worker uploads checkpoints to a secondary directory, S3, Cloudflare R2, or Google Cloud Storage without slowing down the local request path.

Because uploads run asynchronously, a machine crash can lose events written after the last uploaded checkpoint.

## Configuration

Add a `persistence` block to your host config. Using a local `filesystem` provider is the easiest way to test checkpointing and recovery on one machine:

```json
{
  "provider": "filesystem",
  "directory": ".brain-archive",
  "interval_seconds": 5,
  "queue": 128
}
```

### S3 and Cloudflare R2

- **AWS S3:** Set `provider: "s3"`, `bucket`, `region`, and an optional key `prefix`. Brain uses the standard AWS SDK credential chain unless you specify environment variable names in `access_key_env` and `secret_key_env`.
- **Cloudflare R2:** Set `provider: "r2"`, `bucket`, `endpoint` (`https://ACCOUNT_ID.r2.cloudflarestorage.com`), `access_key_env`, and `secret_key_env`. Region defaults to `auto`.
- **PostgreSQL index (optional):** Set `postgres_env` to the name of an environment variable holding a PostgreSQL connection string. PostgreSQL stores an eventually consistent session index for discovery; it does not lock or coordinate session ownership.

The Brain server only needs `GET` and `PUT` access under its configured prefix. Keep internal checkpoint storage separate from any S3/R2 buckets exposed to agents as tools. Brain never archives provider API keys or host credential files—when a session is restored on a new server, credentials come from the new host's configuration.

### Google Cloud Storage

Use `{"provider":"gcs","bucket":"YOUR_BUCKET","prefix":"brain"}`. The official Google SDK reads Application Default Credentials: use a workload identity on Google Cloud or `GOOGLE_APPLICATION_CREDENTIALS` pointing to a credential file locally. GCS rejects S3-specific region, endpoint, and credential fields. Grant object read/create/update under the configured prefix; no bucket administration is needed. Immutable objects use generation preconditions, while the `latest` pointer is replaceable.

All three cloud adapters pass local SDK transport contracts. Live-account qualification is tracked separately; passing a simulated transport test is not evidence of credentials, IAM, or provider availability.

### Hosted PostgreSQL

Neon and Supabase use the existing PostgreSQL adapter, without new runtime dependencies. Put their connection string in the environment variable named by `postgres_env`, with TLS enabled. Use a direct connection or session pool initially; transaction-pool prepared-statement behavior varies by provider. Brain already bounds each process to four database connections. PostgreSQL remains an eventual discovery index, not session ownership coordination.

## How checkpoints work

Brain uploads transcript data in immutable 64 KiB blocks alongside bounded tail segments and native harness files. It writes the checkpoint manifest only after every referenced file is in place, then updates the `latest` pointer and optional PostgreSQL index. Already uploaded 64 KiB blocks are reused on subsequent turns instead of re-uploading the full conversation history.

On startup, Brain scans local disk and queues any unsynced sessions. Transient storage failures retry up to five times with exponential backoff, while permanent configuration or credential errors stop immediately. If the sync queue fills up, Brain rejects new input (`429`) and marks `/health/ready` as `503 not_ready` until the backlog drains.

### Flushing and checking status

- `GET /sessions/{id}/sync` returns the latest published checkpoint, pending upload state, and any sync error.
- `POST /sessions/{id}/flush` (requires `manage` permission) forces an immediate upload of local event history and the latest completed native checkpoint:
  - `checkpoint`: immutable reference for handoff or restore.
  - `history_bytes`: uploaded event log offset.
  - `resume_offset` and `native_checkpoint`: the boundary where the harness can resume execution.
  - `time`: publication timestamp.

If you flush while a turn is still running, `history_bytes` may include events beyond `resume_offset`. Flushing does not pause or trigger model execution.

## Moving a session to another server

Brain does not run distributed consensus or merge writes from two servers running the same session. Your platform must stop the old owner before starting the new one:

1. Stop routing new input to the old instance and let the current turn finish (or interrupt it).
2. Call `POST /sessions/{id}/flush` on the old instance and save the returned `checkpoint` reference.
3. Stop the old owner and route the session to the new instance (which must have the same runtime version, harness config, tool bindings, and storage access).
4. Call `POST /sessions/{id}/restore` on the new instance with `{"checkpoint": "RETURNED_REFERENCE"}`.
5. Call `POST /sessions/{id}/resume`, then send new input.

When a server receives `GET /sessions/{id}` for a session that isn't on local disk, it automatically hydrates the latest published checkpoint from external storage. During a planned handoff, always call `POST /sessions/{id}/restore` with the explicit `checkpoint` ID from step 2 so a stale local cache or delayed upload cannot pick an older boundary.

When `restore` replaces an existing local directory, Brain renames the old directory to `SESSION_ID-replaced-ID` so operators can inspect any unsynced local tail before deleting it.

## Operational notes

- Restoring a session recovers its transcript and native model context; it does not roll back or restart external tool jobs, sandboxes, or browser sessions.
- Keep object lifecycle policies from expiring blocks referenced by retained checkpoints.
