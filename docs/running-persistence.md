# Persistence and recovery

External persistence is an optional background archive that uploads local session checkpoints and event logs to a secondary filesystem directory, AWS S3, Cloudflare R2, or Google Cloud Storage (`persistence` in `brain.json`).

Brain always keeps active session state on local disk first (`metadata.sqlite` for session records, plus append-only `events.jsonl` and `native/` files in `data_dir`) so turns never block on remote storage.

> **Note:** Because uploads run asynchronously in the background, an ungraceful machine crash can lose events written after the last uploaded checkpoint.

## Before you begin

- A working Brain configuration file (`brain.json`).
- Write access to a local archive directory or a bucket in AWS S3, Cloudflare R2, or Google Cloud Storage.
- Optional: a PostgreSQL connection string (`postgres_env`) if you need cross-instance cold-session discovery (`GET /sessions`).

## Configure a storage provider

Add a `persistence` block to your host configuration file. Using the `filesystem` provider is the fastest way to test checkpointing and recovery on a single machine:

```json
{
  "provider": "filesystem",
  "directory": ".brain-archive",
  "interval_seconds": 5,
  "queue": 128
}
```

### Object storage providers

| Provider | Required `persistence` fields | Optional `persistence` fields | Authentication & notes |
| --- | --- | --- | --- |
| `filesystem` | `provider`, `directory` | `prefix`, `postgres_env`, `interval_seconds` (`5`), `queue` (`128`) | Writes immutable hard-linked blocks and manifests to a local or mounted directory. |
| `s3` | `provider`, `bucket` | `region`, `endpoint`, `prefix`, `access_key_env`, `secret_key_env`, `postgres_env`, `interval_seconds`, `queue` | Uses the standard AWS SDK credential chain unless both `access_key_env` and `secret_key_env` are set. |
| `r2` | `provider`, `bucket`, `endpoint`, `access_key_env`, `secret_key_env` | `region` (defaults to `"auto"`), `prefix`, `postgres_env`, `interval_seconds`, `queue` | Set `endpoint` to `https://ACCOUNT_ID.r2.cloudflarestorage.com`. |
| `gcs` | `provider`, `bucket` | `prefix`, `postgres_env`, `interval_seconds`, `queue` | Uses Google Application Default Credentials (workload identity on Google Cloud or `GOOGLE_APPLICATION_CREDENTIALS` locally). Rejects `region`, `endpoint`, `access_key_env`, and `secret_key_env`. |

- **Least-privilege permissions:** The Brain server only needs object read/write (`GET` and `PUT`, or object read/create/update on GCS) under its configured `prefix`; no bucket administration permissions are required.
- **Bucket isolation:** Keep internal checkpoint storage separate from any S3/R2 buckets exposed to agents as tools.
- **No credential archiving:** Brain never archives provider API keys or host credential files. When a session is restored on a new server, model and tool credentials come from the new host's configuration.

> **Note:** All three cloud adapters (`s3`, `r2`, and `gcs`) pass local SDK transport contract tests. Live-account qualification is tracked separately; passing a simulated transport test is not evidence of credentials, IAM, or provider availability.

### Optional PostgreSQL discovery index (`postgres_env`)

Set `postgres_env` to the name of an environment variable holding a PostgreSQL connection string (`postgres://...`) to maintain an eventually consistent session index across instances:

- **Hosted PostgreSQL (Neon, Supabase, RDS):** Put the provider's connection string in the environment variable named by `postgres_env` with TLS enabled. Use a direct connection or session pool initially, because transaction-pool prepared-statement behavior varies by provider. Brain bounds each process to at most 4 open database connections.
- **Discovery only:** PostgreSQL stores an eventually consistent session index for `GET /sessions` and cold-session lookup; it does not lock or coordinate session ownership.

## How checkpoints work

Brain uploads `events.jsonl` and append-only `native/` harness files in immutable `64 KiB` blocks alongside bounded tail segments. It writes the checkpoint manifest (`sessions/<id>/checkpoints/<checkpoint_id>`) only after every referenced block is in place, then updates the mutable `sessions/<id>/latest` pointer (using generation preconditions on immutable objects and replaceable writes on `latest`) and optional PostgreSQL index. Already uploaded 64 KiB blocks are reused on subsequent turns instead of re-uploading the full conversation history.

On startup, Brain scans local disk and queues any unsynced sessions. Transient storage failures retry up to 5 times with exponential backoff, while permanent configuration or credential errors stop immediately. If the sync queue (`queue`, default `128`) fills up, Brain rejects new writes with `429 overloaded` and marks `GET /health/ready` as `503 not_ready` until the backlog drains.

### Flush and inspect sync status

- **`GET /sessions/{id}/sync`** returns the latest published checkpoint status, pending upload state, and any sync error.
- **`POST /sessions/{id}/flush`** (requires `manage: true`) forces an immediate upload of local event history and the latest completed native checkpoint and returns a `SyncStatus` object:

| Field | Description |
| --- | --- |
| `checkpoint` | Immutable 32-character hex checkpoint reference for handoff or restore. |
| `history_bytes` | Uploaded `events.jsonl` byte offset. |
| `resume_offset` | Event log byte offset corresponding to the committed native checkpoint boundary. |
| `native_checkpoint` | Identifier of the committed native harness checkpoint. |
| `time` | UTC publication timestamp of the checkpoint manifest. |
| `pending` | `true` if newer local state still needs to be uploaded. |
| `error` | Last background sync error message, if any. |

> **Note:** If you call `POST /sessions/{id}/flush` while a turn is still running, `history_bytes` may include events written beyond `resume_offset`. Flushing does not pause or trigger model execution.

## Move a session to another server

Brain does not run distributed consensus or merge writes from two servers running the same session. Your platform must stop the old owner before starting the new one:

### 1. Quiesce the old instance

Stop routing new input to the old instance and let the current turn finish (or send `user.interrupt` via `POST /sessions/{id}/events` and wait for the session to settle).

### 2. Flush and record the checkpoint ID

Call `POST /sessions/{id}/flush` on the old instance and save the returned `checkpoint` reference from the response.

### 3. Re-route to the new instance

Stop the old owner and route the session to the new instance. The new instance must have the same runtime version, harness configuration, tool bindings, and external storage access.

### 4. Restore the explicit checkpoint

Call `POST /sessions/{id}/restore` on the new instance with `{"checkpoint": "RETURNED_REFERENCE"}`.

### 5. Resume the session

Call `POST /sessions/{id}/resume` to move the restored session from `interrupted` to `idle`, then send new input.

When a server receives `GET /sessions/{id}` or `GET /sessions/{id}/events` for a session that is not on local disk, it automatically hydrates the latest published checkpoint from external storage. During a planned handoff, always call `POST /sessions/{id}/restore` with the explicit `checkpoint` ID from step 2 so a stale local cache or delayed upload cannot pick an older boundary.

> **Note:**
> - When `restore` replaces an existing local session directory, Brain renames the old directory to `SESSION_ID-replaced-ID` so operators can inspect any unsynced local tail before deleting it.
> - Restoring a session recovers its transcript and native model context; it does not roll back or restart external tool jobs, sandboxes, or browser sessions.
> - Configure bucket object lifecycle policies so they never expire 64 KiB blocks referenced by retained checkpoints.

## Next steps

- [Host configuration reference](https://aexhq.github.io/brain/docs/reference/configuration)
- [Persistence adapters](https://aexhq.github.io/brain/docs/adapters/persistence)
- [Operations & observability](https://aexhq.github.io/brain/docs/guides/operations)
- [HTTP API reference](https://aexhq.github.io/brain/docs/api)
