"""Regenerates contracts/examples/** — worked examples of every message type.

They are the conformance corpus: every file is validated against the schema and round-tripped
through the generated Rust and TypeScript types in CI. Edit this script, not the JSON files.
"""
import hashlib
import json
import os
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
H = "a" * 64
HAND_MANIFEST = json.loads((ROOT / "contracts/abi/v1/tools/manifest.json").read_text(encoding="utf-8"))
EXAMPLE_HAND_MANIFEST = {"version": "1", "tools": [HAND_MANIFEST["tools"][0]]}


def native_hand_tool(name):
    spec = next(tool for tool in HAND_MANIFEST["tools"] if tool["name"] == name)
    return {
        "definition": {key: spec[key] for key in ("name", "description", "input_schema", "output_schema")},
        "executor": {"kind": "hand", **spec["executable"]},
    }


def jcs_sha256(o):
    """SHA-256 over the RFC 8785 canonical JSON. Python's compact, key-sorted dump is JCS for the
    integer/string-only values used here (no floats)."""
    canonical = json.dumps(o, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def start_call_hash(args):
    """Same formula as brain_protocol::tools::call_hash: {tool,input,lane,cwd,detach,bounds}."""
    return jcs_sha256({"tool": args["tool"], "input": args["input"], "lane": args["lane"],
                       "cwd": args.get("cwd"), "detach": args["detach"], "bounds": args.get("bounds")})


def w(p, o):
    if isinstance(o, dict) and o.get("call", {}).get("op") == "start":
        o["call"]["args"]["call_hash"] = start_call_hash(o["call"]["args"])
    p = ROOT / p
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps(o, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")


A = "contracts/examples/abi/"
w(A + "Request.hello-fresh.json", {"id": "r-1", "fence": 7, "call": {"op": "hello", "args": {
    "protocol": {"major": 1, "minor": 0}, "session_id": "ses_01HZX8Y2K3M4N5P6Q7R8S9T0", "session_token": "tok_example",
    "tool_manifest_digest": H, "tool_manifest": EXAMPLE_HAND_MANIFEST, "env": {"GIT_AUTHOR_NAME": "agent"},
    "sync": {"roots": ["/workspace", "/home/agent"], "exclude": ["**/.cache/**", "**/node_modules/.cache/**"]},
    "heartbeat_ms": 5000}}})
w(A + "Request.hello-restore.json", {"id": "r-1", "fence": 9, "call": {"op": "hello", "args": {
    "protocol": {"major": 1, "minor": 0}, "session_id": "ses_01HZX8Y2K3M4N5P6Q7R8S9T0", "session_token": "tok_example",
    "expected_generation_id": "gen-2", "tool_manifest_digest": H, "tool_manifest": EXAMPLE_HAND_MANIFEST, "env": {},
    "sync": {"roots": ["/workspace", "/home/agent"]},
    "restore": {"manifest_id": "m-12", "manifest_get_url": "https://s3.example/ses/m-12.json?X-Amz-Signature=x",
                "packs": [{"pack_id": "p-3", "get_url": "https://s3.example/ses/p-3.tar.zst?X-Amz-Signature=x"},
                          {"pack_id": "p-12", "get_url": "https://s3.example/ses/p-12.tar.zst?X-Amz-Signature=x"}]},
    "heartbeat_ms": 5000}}})
w(A + "Request.start-attached.json", {"id": "r-2", "fence": 9, "generation_id": "gen-3", "call": {"op": "start", "args": {
    "operation_id": "op-0001", "call_hash": H, "batch_id": "b-1", "tool": "bash", "input": {"command": "cargo test 2>&1 | tail -20"},
    "lane": {"id": "0", "mode": "persistent"}, "cwd": "/workspace/repo", "detach": False, "wait_ms": 30000, "max_bytes": 65536,
    "bounds": {"timeout_ms": 600000, "grace_ms": 2000}, "correlation": {"agent_id": "root", "tool_call_id": "call_abc"}}}})
w(A + "Request.start-detached-ephemeral.json", {"id": "r-3", "fence": 9, "generation_id": "gen-3", "call": {"op": "start", "args": {
    "operation_id": "op-0002", "call_hash": H, "batch_id": "b-2", "tool": "bash", "input": {"command": "npm run dev", "timeout_ms": None},
    "lane": {"id": "L-7", "mode": "ephemeral", "parent": "0"}, "detach": True, "wait_ms": 0, "max_bytes": 0}}})
w(A + "Request.poll.json", {"id": "r-4", "fence": 9, "generation_id": "gen-3", "call": {"op": "poll", "args": {
    "operation_id": "op-0002", "cursors": [{"stream": "stdout", "offset": 4096}, {"stream": "stderr", "offset": 0}], "max_bytes": 65536, "wait_ms": 10000}}})
w(A + "Request.cancel.json", {"id": "r-5", "fence": 9, "generation_id": "gen-3", "call": {"op": "cancel", "args": {"operation_id": "op-0002", "grace_ms": 5000}}})
w(A + "Request.release.json", {"id": "r-6", "fence": 9, "generation_id": "gen-3", "call": {"op": "release", "args": {"operation_ids": ["op-0001", "op-0002"]}}})
w(A + "Request.lane_close.json", {"id": "r-7", "fence": 9, "generation_id": "gen-3", "call": {"op": "lane_close", "args": {"lane_id": "L-7"}}})
w(A + "Request.put.json", {"id": "r-8", "fence": 9, "generation_id": "gen-3", "call": {"op": "put", "args": {"files": [
    {"path": "/workspace/input/data.csv", "source": {"kind": "url", "get_url": "https://s3.example/in/data.csv?X-Amz-Signature=x", "bytes": 1048576, "sha256": H}},
    {"path": "/workspace/.brain/instructions.md", "source": {"kind": "inline", "data_base64": "IyBIZWxsbwo="}, "mode": 420}]}}})
w(A + "Request.persist.json", {"id": "r-9", "fence": 9, "generation_id": "gen-3", "call": {"op": "persist", "args": {"items": [
    {"name": "report.pdf", "source": {"kind": "path", "path": "/workspace/out/report.pdf"}, "put_url": "https://s3.example/art/report.pdf?X-Amz-Signature=x", "media_type": "application/pdf"},
    {"name": "build.log", "source": {"kind": "operation_stream", "operation_id": "op-0001", "stream": "stdout"}, "put_url": "https://s3.example/art/build.log?X-Amz-Signature=x"}]}}})
w(A + "Request.sync.json", {"id": "r-10", "fence": 9, "generation_id": "gen-3", "call": {"op": "sync", "args": {
    "reason": "turn_end", "manifest_id": "m-13", "manifest_put_url": "https://s3.example/ses/m-13.json?X-Amz-Signature=x",
    "pack_id": "p-13", "pack_put_url": "https://s3.example/ses/p-13.tar.zst?X-Amz-Signature=x", "full": False}}})

view_running = {"operation_id": "op-0002", "tool": "bash", "lane_id": "L-7", "detach": True, "status": "running", "started_at_monotonic_ms": 120345,
                "streams": [{"stream": "stdout", "produced_bytes": 8192, "retained_from": 0, "spill_path": "/var/hand/ops/op-0002.stdout"},
                            {"stream": "stderr", "produced_bytes": 0, "retained_from": 0}]}
view_done = {"operation_id": "op-0001", "tool": "bash", "lane_id": "0", "detach": False, "status": "terminal", "started_at_monotonic_ms": 100000,
             "terminal": {"outcome": "completed", "exit_code": 1, "signal": None, "output": {"timed_out": False}, "ended_at_monotonic_ms": 104210,
                          "usage": {"wall_ms": 4210, "cpu_ms": 3900, "max_rss_bytes": 734003200}},
             "streams": [{"stream": "stdout", "produced_bytes": 2210, "retained_from": 0, "spill_path": "/var/hand/ops/op-0001.stdout", "sha256": H},
                         {"stream": "stderr", "produced_bytes": 0, "retained_from": 0, "sha256": H}],
             "correlation": {"agent_id": "root", "tool_call_id": "call_abc"}}
w(A + "HandFrame.response-hello.json", {"kind": "response", "frame": {"id": "r-1", "result": {"status": "ok", "reply": {"op": "hello", "body": {
    "protocol": {"major": 1, "minor": 0}, "generation_id": "gen-3", "boot_id": "boot-9f3a", "tool_manifest_digest": H,
    "tools": EXAMPLE_HAND_MANIFEST["tools"],
    "lanes": [{"id": "0", "mode": "persistent", "state": "live", "created_at_monotonic_ms": 812}],
    "operations": [],
    "limits": {"max_lanes": 64, "max_concurrent_operations": 64, "max_frame_bytes": 1048576, "max_slice_bytes": 262144, "max_poll_wait_ms": 30000,
               "max_inline_put_bytes": 262144, "max_persist_bytes": 1073741824,
               "default_bounds": {"timeout_ms": None, "grace_ms": 2000, "max_retained_bytes": 67108864}},
    "paths": {"workspace": "/workspace", "home": "/home/agent", "spill_dir": "/var/hand/ops"},
    "clock": {"monotonic_ms": 812, "wall_ms": 1787400000000},
    "restore": {"manifest_id": "m-12", "files": 1432, "bytes": 16777216, "duration_ms": 1080}}}}}})
w(A + "HandFrame.response-start.json", {"kind": "response", "frame": {"id": "r-2", "result": {"status": "ok", "reply": {"op": "start", "body": {
    "view": view_done, "slices": [{"stream": "stdout", "offset": 0, "data_base64": "dGVzdCByZXN1bHQ6IEZBSUxFRC4gMSBwYXNzZWQ7IDEgZmFpbGVkCg==", "eof": True}], "replayed": False}}}}})
w(A + "HandFrame.response-poll.json", {"kind": "response", "frame": {"id": "r-4", "result": {"status": "ok", "reply": {"op": "poll", "body": {
    "view": view_running, "slices": [{"stream": "stdout", "offset": 4096, "data_base64": "cmVhZHkgb24gaHR0cDovL2xvY2FsaG9zdDozMDAwCg==", "eof": True},
                                     {"stream": "stderr", "offset": 0, "data_base64": "", "eof": True}]}}}}})
w(A + "HandFrame.response-cancel.json", {"kind": "response", "frame": {"id": "r-5", "result": {"status": "ok", "reply": {"op": "cancel", "body": {"accepted": True, "view": view_running}}}}})
w(A + "HandFrame.response-release.json", {"kind": "response", "frame": {"id": "r-6", "result": {"status": "ok", "reply": {"op": "release", "body": {"released": ["op-0001"], "unknown": ["op-0002"]}}}}})
w(A + "HandFrame.response-lane_close.json", {"kind": "response", "frame": {"id": "r-7", "result": {"status": "ok", "reply": {"op": "lane_close", "body": {"closed": True, "cancelled_operations": []}}}}})
w(A + "HandFrame.response-put.json", {"kind": "response", "frame": {"id": "r-8", "result": {"status": "ok", "reply": {"op": "put", "body": {"written": [
    {"path": "/workspace/input/data.csv", "bytes": 1048576, "sha256": H}, {"path": "/workspace/.brain/instructions.md", "bytes": 8, "sha256": H}]}}}}})
w(A + "HandFrame.response-persist.json", {"kind": "response", "frame": {"id": "r-9", "result": {"status": "ok", "reply": {"op": "persist", "body": {"persisted": [
    {"name": "report.pdf", "bytes": 204800, "sha256": H, "media_type": "application/pdf"}, {"name": "build.log", "bytes": 2210, "sha256": H, "media_type": "text/plain"}]}}}}})
w(A + "HandFrame.response-sync.json", {"kind": "response", "frame": {"id": "r-10", "result": {"status": "ok", "reply": {"op": "sync", "body": {
    "changed": True, "manifest_id": "m-13", "files_total": 1440, "bytes_total": 17825792, "files_added": 6, "files_modified": 2, "files_deleted": 1,
    "bytes_uploaded": 48211, "packs_referenced": 3, "duration_ms": 412}}}}})
w(A + "HandFrame.response-error.json", {"kind": "response", "frame": {"id": "r-2", "result": {"status": "error", "error": {
    "code": "tool_input_invalid", "message": "input.command: expected string, got null", "retryable": False, "details": {"schema_path": "/properties/command/type"}}}}})
w(A + "HandFrame.hand_status.json", {"kind": "hand_status", "frame": {"generation_id": "gen-3", "boot_id": "boot-9f3a", "seq": 42, "at_monotonic_ms": 220000, "at_wall_ms": 1787400220000,
    "inflight": [], "live_jobs": ["op-0002"], "lanes_live": 2, "operations_retained": 1, "retained_bytes": 2210, "idle_for_ms": 0,
    "pressure": {"mem_available_bytes": 2147483648, "swap_used_bytes": 0, "psi_some_avg10": 0.0}}})
w(A + "SyncManifest.example.json", {"version": 1, "manifest_id": "m-13", "parent_manifest_id": "m-12", "created_at_wall_ms": 1787400300000, "generation_id": "gen-3",
    "roots": ["/workspace", "/home/agent"], "pack_format": "tar+zstd",
    "packs": [{"pack_id": "p-3", "bytes": 15728640, "sha256": H}, {"pack_id": "p-12", "bytes": 98304, "sha256": H}, {"pack_id": "p-13", "bytes": 48211, "sha256": H}],
    "entries": [{"kind": "dir", "path": "/workspace/repo", "mode": 493},
                {"kind": "file", "path": "/workspace/repo/src/main.rs", "size": 1042, "mtime_ns": 1787400299123456789, "mode": 420, "sha256": H, "pack_id": "p-13"},
                {"kind": "symlink", "path": "/workspace/repo/latest", "target": "target/debug/app"}]})

S = "contracts/examples/session/"
sess = {"id": "ses_01HZX8Y2K3M4N5P6Q7R8S9T0", "object": "session", "state": "idle", "model": {"provider": "anthropic", "name": "claude-sonnet-5"},
        "hand": {"state": "suspended", "shape": "1gb", "generation": 3, "started_at": "2026-08-18T09:00:00Z", "wall_deadline_at": "2026-08-18T17:00:00Z",
                 "last_sync_at": "2026-08-18T09:42:10Z", "live_jobs": 0},
        "storage": {"workspace_bytes": 17825792, "suspended_bytes": 1073741824, "artifact_bytes": 204800},
        "created_at": "2026-08-18T09:00:00Z", "updated_at": "2026-08-18T09:42:10Z", "last_message_at": "2026-08-18T09:40:00Z", "turns": 4,
        "metadata": {"customer_ref": "job-77"}}
w(S + "Session.idle.json", sess)
w(S + "CreateSessionRequest.full.json", {"model": {"provider": "anthropic", "name": "claude-sonnet-5", "api_key": "sk-ant-REDACTED", "max_output_tokens": 8192},
    "system_prompt": "You are a careful engineer.",
    "tools": {"items": [
                  native_hand_tool("bash"),
                  {"definition": {"name": "task", "description": "Delegate a bounded task to a child agent.",
                                  "input_schema": {"type": "object", "properties": {"description": {"type": "string"}, "prompt": {"type": "string"}},
                                                   "required": ["description", "prompt"], "additionalProperties": False},
                                  "output_schema": {"type": "object", "additionalProperties": True}},
                   "executor": {"kind": "intrinsic", "capability": "brain.subagents.v1"}},
                  {"definition": {"name": "host_result", "description": "Return a result to the host.",
                                  "input_schema": {"type": "object", "additionalProperties": True},
                                  "output_schema": {"type": "object", "additionalProperties": True}},
                   "executor": {"kind": "server", "capability": "example.result.v1", "scope": "root",
                                "completion": "return_direct", "effect": "replay_safe", "max_input_bytes": 98304}}
              ],
              "mcp": [{"name": "github", "url": "https://mcp.example.com/github", "headers": {"Authorization": "Bearer REDACTED"}, "protocol": "auto"}]},
    "hand": {"shape": "2gb", "env": {"GH_TOKEN": "REDACTED"}, "sync_interval_seconds": 600},
    "files": [{"path": "README.md", "content_base64": "IyBIZWxsbwo="}], "metadata": {"customer_ref": "job-77"}})
w(S + "CreateSessionRequest.minimal.json", {"model": {"provider": "openai", "name": "gpt-5", "api_key": "sk-REDACTED"}})
w(S + "MessageRequest.text.json", {"content": "Run the test suite and fix the failing test."})
w(S + "MessageRequest.parts.json", {"content": [{"type": "text", "text": "Summarise this file."}, {"type": "workspace_file", "path": "/workspace/README.md"}]})
request_id = "req_01HZX8Y2K3M4N5P6Q7R8S9V2"
w(S + "MessageAccepted.example.json", {"session_id": "ses_01HZX8Y2K3M4N5P6Q7R8S9T0", "turn_id": "trn_01HZX8Y2K3M4N5P6Q7R8S9U1", "seq": 118})
w(S + "ExternalToolCallRequest.example.json", {
    "session_id": "ses_01HZX8Y2K3M4N5P6Q7R8S9T0", "turn_id": "trn_01HZX8Y2K3M4N5P6Q7R8S9U1",
    "agent_id": "root", "call_id": "call_01HZX8Y2K3M4N5P6Q7R8S9W3", "name": "host_result",
    "input": {"recommendation": "Option A", "confidence": 0.86},
    "context": {"host.request_id": request_id},
})
w(S + "ExternalToolCallResponse.complete.json", {
    "outcome": "completed", "content": "The result was accepted.", "is_error": False,
    "disposition": "complete_turn", "result": {"recommendation": "Option A", "confidence": 0.86},
    "result_metadata": {"request_id": request_id, "validator": "recommendation-v1"},
})
base = {"session_id": "ses_01HZX8Y2K3M4N5P6Q7R8S9T0", "turn_id": "trn_01HZX8Y2K3M4N5P6Q7R8S9U1"}


def ev(seq, t, **k):
    return {"type": t, "seq": seq, "at": "2026-08-18T09:40:%02dZ" % (seq % 60), **base, **k}


w(S + "Event.turn.started.json", ev(118, "turn.started"))
w(S + "Event.assistant.delta.json", ev(119, "assistant.delta", agent_id="root", text="I will run"))
w(S + "Event.assistant.message.json", ev(127, "assistant.message", agent_id="root", text="I will run the tests first."))
w(S + "Event.tool.call.json", ev(120, "tool.call", agent_id="root", call_id="op-0001", name="bash", input={"command": "cargo test 2>&1 | tail -20"}, detach=False))
w(S + "Event.tool.output.json", ev(121, "tool.output", call_id="op-0001", stream="stdout", offset=0, text="running 12 tests\n"))
w(S + "Event.tool.result.json", ev(122, "tool.result", agent_id="root", call_id="op-0001", name="bash", outcome="completed", exit_code=1, duration_ms=4210,
                                   output_preview="test result: FAILED. 11 passed; 1 failed", truncated=False))
w(S + "Event.agent.spawned.json", ev(123, "agent.spawned", agent_id="agt_01", parent_agent_id="root", depth=1, description="Investigate the failing test"))
w(S + "Event.agent.finished.json", ev(128, "agent.finished", agent_id="agt_01", outcome="completed", summary="The failing test asserts on a stale fixture."))
w(S + "Event.model.usage.json", ev(124, "model.usage", agent_id="root", provider="anthropic", model="claude-sonnet-5",
                                   usage={"input_tokens": 12034, "output_tokens": 211, "cache_read_input_tokens": 11800}))
w(S + "Event.session.updated.json", {"type": "session.updated", "seq": 130, "at": "2026-08-18T09:45:00Z", "session_id": "ses_01HZX8Y2K3M4N5P6Q7R8S9T0",
                                     "state": "idle", "hand": sess["hand"]})
w(S + "Event.hand.lost.json", {"type": "hand.lost", "seq": 131, "at": "2026-08-18T09:46:00Z", "session_id": "ses_01HZX8Y2K3M4N5P6Q7R8S9T0",
                               "turn_id": "trn_01HZX8Y2K3M4N5P6Q7R8S9U1", "interrupted_calls": ["op-0002"], "workspace_synced_at": "2026-08-18T09:42:10Z"})
w(S + "Event.turn.completed.json", ev(125, "turn.completed", stop_reason="end_turn", rounds=5, tool_calls=7,
                                        result={"call_id": "call_01HZX8Y2K3M4N5P6Q7R8S9W3", "name": "host_result",
                                                "value": {"recommendation": "Option A", "confidence": 0.86},
                                                "metadata": {"request_id": request_id, "validator": "recommendation-v1"}}))
w(S + "Event.turn.failed.json", ev(126, "turn.failed", error={"code": "invalid_request", "message": "The host rejected the terminal tool result",
                                                               "request_id": "req_9x", "details": {"issues": [{"path": "/confidence", "message": "must be less than or equal to 1", "keyword": "maximum"}]}}))
w(S + "ApiErrorResponse.example.json", {"error": {"code": "session_busy", "message": "a turn is already running", "request_id": "req_9y"}})
w(S + "Artifact.example.json", {"object": "artifact", "session_id": "ses_01HZX8Y2K3M4N5P6Q7R8S9T0", "name": "report.pdf", "bytes": 204800, "sha256": H,
                                "media_type": "application/pdf", "created_at": "2026-08-18T09:41:00Z",
                                "download_url": "https://s3.example/art/report.pdf?X-Amz-Signature=x", "download_url_expires_at": "2026-08-18T10:41:00Z"})
w(S + "FileList.manifest.json", {"object": "list", "data": [{"path": "/workspace/repo/src/main.rs", "kind": "file", "size": 1042, "modified_at": "2026-08-18T09:41:39Z", "sha256": H},
                                                            {"path": "/workspace/repo/target", "kind": "dir"}],
                                 "synced_at": "2026-08-18T09:42:10Z", "source": "manifest"})
w(S + "SessionList.example.json", {"object": "list", "data": [sess], "has_more": False})
w(S + "PersistRequest.example.json", {"name": "report.pdf", "path": "/workspace/out/report.pdf", "media_type": "application/pdf"})

print("examples written:", len(os.listdir(ROOT / A)), len(os.listdir(ROOT / S)))
