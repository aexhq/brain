# Persistence and recovery

External storage is optional. Without it, Brain uses SQLite and local files only. With it, input still completes after local acceptance; a bounded worker uploads checkpoints asynchronously. Machine loss can lose the unsynchronized tail.

Add a `persistence` object to host configuration. A second local directory exercises the same publication and recovery path:

```json
{
  "provider": "filesystem",
  "directory": ".brain-archive",
  "interval_seconds": 5,
  "queue": 128
}
```

For AWS, use `provider: "s3"`, `bucket`, `region`, and an optional `prefix`. The AWS SDK's standard credential chain is available. Explicit credentials use `access_key_env` and `secret_key_env`, containing environment-variable names. For Cloudflare, use `provider: "r2"`, a bucket, `endpoint: "https://ACCOUNT_ID.r2.cloudflarestorage.com"`, and those explicit credential names. R2 defaults to region `auto`. The same host configuration can set `postgres_env` to the name of an environment variable holding a PostgreSQL connection string. PostgreSQL stores an eventual session index; it does not coordinate ownership.

The host needs object GET/PUT access to its configured prefix. Model-facing storage tools should use separate credentials and resource scopes. Native credential/configuration directories are not archived; the adapter supplies only the files required to restore its conversation. Credentials are rebound from host configuration on the new server.

`POST /sessions/{id}/flush` captures local event history and the latest completed native checkpoint, then returns:

- `checkpoint`: an immutable external reference for handoff or disaster recovery.
- `history_bytes`: the uploaded event boundary.
- `resume_offset` and `native_checkpoint`: the corresponding usable native boundary.
- `time`: publication time.

History can be newer than the native checkpoint. A successful flush during a running turn does not mean that turn can be resumed halfway through. Flush never activates or pauses inference. `GET /sessions/{id}/sync` reports the last publication, pending work, and synchronization errors. These operations require configured external storage; flush requires management access.

Uploads use immutable 64 KiB blocks, bounded partial tails, and a small checkpoint manifest. A manifest is published only after all its files are uploaded. The latest pointer and optional PostgreSQL reference follow publication. Complete blocks are reused without hashing or uploading the growing transcript every turn. Only adapters with a proven append-only native checkpoint are currently supported by this path.

The queue contains bounded references to local data. Startup scans rediscover pending state. Storage failures retry with backoff for at most five attempts; known credential/bucket failures stop immediately. Pending data remains local. A full backlog rejects new input and changes readiness to unavailable. Explicit flush surfaces an error directly and allows an operator to retry after correcting the cause. Tool actions are never retried by this worker.

For a planned handoff:

1. Stop routing new input to the old owner and wait for a completed native boundary. Interruption may leave newer history beyond that boundary.
2. Flush and retain the returned immutable checkpoint reference.
3. Stop the old owner. Route the session to a new instance with the same runtime artifact, harness/provider configuration, bindings, and storage access.
4. Call `POST /sessions/{id}/restore` with `{"checkpoint":"RETURNED_REFERENCE"}`. An active request, stream, or execution must settle before replacing an existing cache.
5. Call `POST /sessions/{id}/resume`, then submit new input. Restore and resume alone make no model requests.

When local metadata is absent, reading a known session automatically hydrates its latest complete external checkpoint. Use an explicit checkpoint during reassignment so an old cache or delayed publication cannot select the wrong boundary. Explicit restore also replaces an unusable existing cache. Replaced local directories are retained as `SESSION_ID-replaced-ID` for operator recovery; remove them only after deciding whether their unsynced tail is needed.

Brain does not merge concurrent owners. The surrounding platform must quiesce the previous owner. Remote tool side effects, jobs, browsers, and resources remain external. Restoring transcript state does not undo or restart those actions.

Current automated evidence includes bounded append upload, failed publication, missing blocks, tenant/path rejection, backlog admission, passive restore, and the next Pi model request after full cache relocation. PostgreSQL runs in CI. S3/R2 SDK transport tests are not live cloud-account qualification. Cloud lifecycle policies, removal of unreachable failed-upload objects, regional replication, and retention remain operator choices; do not expire objects referenced by retained checkpoints.
