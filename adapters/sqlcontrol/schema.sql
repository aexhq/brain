CREATE TABLE IF NOT EXISTS brain_schema (version BIGINT PRIMARY KEY);
INSERT INTO brain_schema(version) SELECT 12 WHERE NOT EXISTS(SELECT 1 FROM brain_schema);
CREATE TABLE IF NOT EXISTS nodes (
 id TEXT PRIMARY KEY, lease BIGINT NOT NULL, draining BIGINT NOT NULL DEFAULT 0,
 endpoint TEXT NOT NULL DEFAULT '', configuration_digest TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS budgets (
 id TEXT PRIMARY KEY, calls BIGINT NOT NULL DEFAULT 0, active BIGINT NOT NULL DEFAULT 0,
 reserved BIGINT NOT NULL DEFAULT 0, retained BIGINT NOT NULL DEFAULT 0,
 tokens BIGINT NOT NULL DEFAULT 0, max_calls BIGINT NOT NULL, max_active BIGINT NOT NULL,
 max_bytes BIGINT NOT NULL, max_tokens BIGINT NOT NULL,
 reserved_tokens BIGINT NOT NULL DEFAULT 0, incomplete_usages BIGINT NOT NULL DEFAULT 0,
 cancel_generation BIGINT NOT NULL DEFAULT 1, paused BIGINT NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS tree_authority(root_id TEXT PRIMARY KEY REFERENCES budgets(id));
CREATE TABLE IF NOT EXISTS budget_members(
 session_id TEXT NOT NULL, budget_id TEXT NOT NULL REFERENCES budgets(id), depth BIGINT NOT NULL,
 generation BIGINT NOT NULL,
 PRIMARY KEY(session_id,budget_id)
);
CREATE INDEX IF NOT EXISTS budget_descendants ON budget_members(budget_id,session_id);
CREATE TABLE IF NOT EXISTS control_cascades(
 session_id TEXT PRIMARY KEY, generation BIGINT NOT NULL, after_id TEXT NOT NULL DEFAULT '',
 status TEXT NOT NULL DEFAULT 'pending'
);
CREATE INDEX IF NOT EXISTS control_cascades_pending ON control_cascades(status,session_id);
CREATE TABLE IF NOT EXISTS sessions (
 id TEXT PRIMARY KEY, tenant TEXT NOT NULL, create_key TEXT NOT NULL, create_hash TEXT NOT NULL,
 config TEXT NOT NULL, lifecycle TEXT NOT NULL DEFAULT 'open', revision BIGINT NOT NULL DEFAULT 1,
 sequence BIGINT NOT NULL DEFAULT 0, owner TEXT NOT NULL DEFAULT '', epoch BIGINT NOT NULL DEFAULT 0,
 generation BIGINT NOT NULL DEFAULT 1, paused BIGINT NOT NULL DEFAULT 0, created_ms BIGINT NOT NULL,
 parent TEXT NOT NULL DEFAULT '', budget TEXT NOT NULL,
 ready BIGINT NOT NULL DEFAULT 0, ready_ms BIGINT NOT NULL DEFAULT 0, UNIQUE(tenant, create_key)
);
CREATE INDEX IF NOT EXISTS sessions_tenant ON sessions(tenant,id);
CREATE INDEX IF NOT EXISTS sessions_owner ON sessions(owner,lifecycle);
CREATE TABLE IF NOT EXISTS commands (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), request_key TEXT NOT NULL,
 digest TEXT NOT NULL, payload TEXT NOT NULL, source TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'accepted',
 accepted_ms BIGINT NOT NULL, run_id TEXT NOT NULL DEFAULT '', sequence BIGINT NOT NULL DEFAULT 0,
 error TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL DEFAULT '', UNIQUE(session_id,request_key)
);
CREATE INDEX IF NOT EXISTS commands_pending ON commands(status,session_id,accepted_ms,id);
CREATE TABLE IF NOT EXISTS runs (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), command_id TEXT NOT NULL,
 status TEXT NOT NULL, step BIGINT NOT NULL DEFAULT 0, checkpoint TEXT NOT NULL DEFAULT '{}',
 decision TEXT NOT NULL DEFAULT '', wait_kind TEXT NOT NULL DEFAULT '', wait_id TEXT NOT NULL DEFAULT '',
 error TEXT NOT NULL DEFAULT '', updated_ms BIGINT NOT NULL,
 completion_sequence BIGINT NOT NULL DEFAULT 0, completion_digest TEXT NOT NULL DEFAULT '',
 harness_process TEXT NOT NULL DEFAULT '', decision_started_step BIGINT NOT NULL DEFAULT -1,
 decision_input_hash TEXT NOT NULL DEFAULT '', decision_repeatable BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS runs_session ON runs(session_id,status,id);
CREATE INDEX IF NOT EXISTS runs_page ON runs(session_id,id);
CREATE TABLE IF NOT EXISTS harness_handoffs (
 session_id TEXT NOT NULL REFERENCES sessions(id), request_key TEXT NOT NULL, request_hash TEXT NOT NULL,
 PRIMARY KEY(session_id,request_key)
);
CREATE TABLE IF NOT EXISTS ownership_transfers (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), request_key TEXT NOT NULL,
 request_hash TEXT NOT NULL, source_node TEXT NOT NULL, source_epoch BIGINT NOT NULL, target_node TEXT NOT NULL,
 phase TEXT NOT NULL DEFAULT 'draining', through_sequence BIGINT NOT NULL DEFAULT 0,
 checkpoint_digest TEXT NOT NULL DEFAULT '', run_cursor TEXT NOT NULL DEFAULT '',
 tail_verified BIGINT NOT NULL DEFAULT 0, verified_runs BIGINT NOT NULL DEFAULT 0,
 error TEXT NOT NULL DEFAULT '', updated_ms BIGINT NOT NULL, UNIQUE(session_id,request_key)
);
CREATE UNIQUE INDEX IF NOT EXISTS ownership_transfer_active ON ownership_transfers(session_id)
WHERE phase IN ('draining','verifying');
CREATE INDEX IF NOT EXISTS ownership_transfer_source ON ownership_transfers(source_node,phase,updated_ms,id);
CREATE INDEX IF NOT EXISTS ownership_transfer_target ON ownership_transfers(target_node,phase,updated_ms,id);
CREATE TABLE IF NOT EXISTS child_groups (
 session_id TEXT NOT NULL REFERENCES sessions(id), run_id TEXT NOT NULL REFERENCES runs(id),
 step BIGINT NOT NULL, expected BIGINT NOT NULL CHECK(expected BETWEEN 1 AND 8),
 PRIMARY KEY(session_id,run_id,step)
);
CREATE TABLE IF NOT EXISTS run_children (
 session_id TEXT NOT NULL REFERENCES sessions(id), run_id TEXT NOT NULL REFERENCES runs(id),
 step BIGINT NOT NULL, ordinal BIGINT NOT NULL, child_id TEXT NOT NULL REFERENCES sessions(id),
 command_id TEXT NOT NULL REFERENCES commands(id), PRIMARY KEY(session_id,run_id,step,ordinal),
 UNIQUE(child_id), FOREIGN KEY(session_id,run_id,step) REFERENCES child_groups(session_id,run_id,step)
);
CREATE TABLE IF NOT EXISTS allocations (
 session_id TEXT NOT NULL REFERENCES sessions(id), sequence BIGINT NOT NULL,
 owner TEXT NOT NULL, epoch BIGINT NOT NULL, status TEXT NOT NULL DEFAULT 'reserved',
 digest TEXT NOT NULL DEFAULT '', bytes BIGINT NOT NULL DEFAULT 0,
 archive_key TEXT NOT NULL DEFAULT '', PRIMARY KEY(session_id,sequence)
);
CREATE TABLE IF NOT EXISTS operations (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), run_id TEXT NOT NULL,
 step_key TEXT NOT NULL, kind TEXT NOT NULL, action TEXT NOT NULL, arguments TEXT NOT NULL,
 arguments_digest TEXT NOT NULL, recipient TEXT NOT NULL, revision BIGINT NOT NULL DEFAULT 1,
 status TEXT NOT NULL DEFAULT 'prepared', output_limit BIGINT NOT NULL, deadline_ms BIGINT NOT NULL,
 generation TEXT NOT NULL, result_sequence BIGINT NOT NULL DEFAULT 0,
 result_digest TEXT NOT NULL DEFAULT '', result_body TEXT NOT NULL DEFAULT '',
 usage TEXT NOT NULL DEFAULT '', error TEXT NOT NULL DEFAULT '',
 receipt_hash TEXT NOT NULL DEFAULT '', receipt_key TEXT NOT NULL DEFAULT '',
 claimant TEXT NOT NULL DEFAULT '', claim_key TEXT NOT NULL DEFAULT '',
 approval BIGINT NOT NULL DEFAULT 0, retry_of TEXT NOT NULL DEFAULT '',
 intent_sequence BIGINT NOT NULL DEFAULT 0, intent_digest TEXT NOT NULL DEFAULT '',
 token_reserve BIGINT NOT NULL DEFAULT 0,
 UNIQUE(run_id,step_key)
);
CREATE INDEX IF NOT EXISTS allocations_transfer ON allocations(session_id,status,archive_key,sequence);
CREATE INDEX IF NOT EXISTS operations_session ON operations(session_id,id);
CREATE INDEX IF NOT EXISTS operations_pending ON operations(status,session_id,deadline_ms);
CREATE TABLE IF NOT EXISTS interactions (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), revision BIGINT NOT NULL DEFAULT 1,
 status TEXT NOT NULL DEFAULT 'pending', request TEXT NOT NULL, request_hash TEXT NOT NULL,
 answer TEXT NOT NULL DEFAULT '', answer_key TEXT NOT NULL DEFAULT '', expires_ms BIGINT NOT NULL,
 subject TEXT NOT NULL, subject_digest TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS interactions_session ON interactions(session_id,status,id);
CREATE INDEX IF NOT EXISTS interactions_page ON interactions(session_id,id);
CREATE TABLE IF NOT EXISTS connections (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), template TEXT NOT NULL,
 generation BIGINT NOT NULL DEFAULT 1, status TEXT NOT NULL DEFAULT 'active',
 ownership TEXT NOT NULL, resource_id TEXT NOT NULL DEFAULT '', request_hash TEXT NOT NULL,
 UNIQUE(session_id,template,generation)
);
CREATE TABLE IF NOT EXISTS documents (
 session_id TEXT NOT NULL REFERENCES sessions(id), scope TEXT NOT NULL, key TEXT NOT NULL,
 revision BIGINT NOT NULL, value TEXT NOT NULL, expires_ms BIGINT NOT NULL DEFAULT 0,
 request_key TEXT NOT NULL, request_hash TEXT NOT NULL, PRIMARY KEY(session_id,scope,key)
);
CREATE INDEX IF NOT EXISTS connections_page ON connections(session_id,id);
CREATE TABLE IF NOT EXISTS files (
 session_id TEXT NOT NULL REFERENCES sessions(id), name TEXT NOT NULL, digest TEXT NOT NULL,
 size BIGINT NOT NULL, media_type TEXT NOT NULL, provenance TEXT NOT NULL, PRIMARY KEY(session_id,name)
);
CREATE TABLE IF NOT EXISTS schedules (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), due_ms BIGINT NOT NULL,
 interval_ms BIGINT NOT NULL, status TEXT NOT NULL DEFAULT 'active', request TEXT NOT NULL,
 request_hash TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS schedules_due ON schedules(status,due_ms,id);
CREATE INDEX IF NOT EXISTS schedules_page ON schedules(session_id,id);
CREATE TABLE IF NOT EXISTS participants (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id), subject TEXT NOT NULL,
 permissions TEXT NOT NULL, expires_ms BIGINT NOT NULL, revoked BIGINT NOT NULL DEFAULT 0,
 request_hash TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS coverage (
 session_id TEXT NOT NULL REFERENCES sessions(id), first_sequence BIGINT NOT NULL,
 last_sequence BIGINT NOT NULL, object_key TEXT NOT NULL, digest TEXT NOT NULL, bytes BIGINT NOT NULL,
 generation BIGINT NOT NULL, PRIMARY KEY(session_id,first_sequence,last_sequence,object_key)
);
CREATE INDEX IF NOT EXISTS coverage_read ON coverage(session_id,last_sequence,first_sequence);
CREATE TABLE IF NOT EXISTS packages (
 tenant TEXT NOT NULL, id TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(tenant,id)
);
CREATE TABLE IF NOT EXISTS cluster_configuration(key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS node_capabilities(
 node_id TEXT NOT NULL,capability TEXT NOT NULL,PRIMARY KEY(node_id,capability)
);
CREATE TABLE IF NOT EXISTS session_requirements(
 session_id TEXT NOT NULL,capability TEXT NOT NULL,PRIMARY KEY(session_id,capability)
);
