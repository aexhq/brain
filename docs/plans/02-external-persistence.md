# 02: External persistence plan

- **Release:** R1 ([Roadmap](../roadmap.md))
- **Prerequisite:** Local storage and Pi checkpoint recovery from [01 Local runtime](01-local-runtime.md)

Add optional background checkpoint storage (filesystem, AWS S3, and Cloudflare R2) and an optional PostgreSQL session discovery index without putting remote network writes on the hot turn path.

## Work items

| ID | Scope | Acceptance criteria |
| --- | --- | --- |
| **P1: Archive format** | Define immutable 64 KiB block keys, bounded tail objects, checkpoint manifests, `latest` pointers, and the filesystem archive store. | A manifest is published only after all referenced blocks and native files exist; incomplete uploads are never visible as valid checkpoints. |
| **P2: S3 and R2 stores** | Implement `persistence.Objects` for S3 and Cloudflare R2, reusing already uploaded 64 KiB blocks across checkpoints. | Contract tests cover immutable puts, idempotent retries, missing objects (`ErrNotFound`), prefix scoping, and 2 MiB object size caps. |
| **P3: Background sync worker** | Per-session ordered upload worker, bounded sync queue, exponential backoff (up to 5 tries) for transient errors, and `flush`/`sync` routes. | Transient errors retry; credential/config errors fail immediately; full sync queues reject new inputs (`429`) and mark `/health/ready` as `503`. |
| **P4: PostgreSQL index** | Optional `persistence.Metadata` adapter in PostgreSQL updated after checkpoint publication. | `GET /sessions` and `GET /sessions/{id}` work across instances; tenant isolation holds; index rows never point to unpublished checkpoints. |
| **P5: Restore and handoff** | Automatic hydration on local cache miss (`GET /sessions/{id}`) and explicit checkpoint replacement (`POST /sessions/{id}/restore`). | Restoring into a fresh directory and sending the next message preserves Pi's conversation context; replaced local caches are saved as `SESSION_ID-replaced-ID`. |
| **P6: Failure & outage tests** | Test interrupted uploads, archive outages, queue backpressure, crash recovery, and handoff between server instances. | See [Persistence and recovery](../running-persistence.md) for the operator handoff runbook and guarantees. |

## Design notes

- **Write order:** Local append $\rightarrow$ 64 KiB block and tail uploads $\rightarrow$ native checkpoint file uploads $\rightarrow$ checkpoint manifest upload $\rightarrow$ `latest` pointer + optional PostgreSQL metadata update.
- **History vs. resumable boundary:** Flushing during a running turn can upload events (`history_bytes`) that are newer than the last completed turn (`resume_offset` / `native_checkpoint`). Both offsets are reported in `SyncStatus`.
- **Single owner per session:** External object storage and PostgreSQL do not provide distributed locking. The host platform must stop the old instance before restoring and resuming on a new instance.
