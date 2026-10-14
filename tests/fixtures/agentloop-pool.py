"""Independent multiplexed worker for registry, capacity and native-state tests."""
import json
import os
from pathlib import Path
import sys

pending = {}
sessions = {}
options = None
counter = 0


def send(frame):
    print(json.dumps({"jsonrpc": "2.0", **frame}), flush=True)


for line in sys.stdin:
    frame = json.loads(line)
    method = frame.get("method")
    params = frame.get("params", {})
    if method == "initialize":
        options = params
        send({"id": frame["id"], "result": {"protocol": "brain/harness/v1", "name": "pool-fixture", "state_version": "1"}})
    elif method == "session/open":
        directory = Path(options["state_directory"]) / params["session_id"]
        directory.mkdir(exist_ok=True)
        state = directory / "counter.json"
        sessions[params["session_id"]] = json.loads(state.read_text()) if state.exists() else 0
        send({"id": frame["id"], "result": None})
    elif method == "session/run":
        if params["input"]["configuration"].get("crash"):
            sys.exit(1)
        counter += 1
        callback = f"model:{counter}"
        pending[callback] = frame
        send({"id": callback, "method": "brain/call", "params": {
            "session_id": params["session_id"], "sequence": params["sequence"], "method": "model",
            "input": {"messages": [{"role": "user", "content": [{"type": "text", "text": params["session_id"]}]}]},
        }})
    elif method in ("session/close", "session/delete"):
        sessions.pop(params["session_id"], None)
        if method == "session/delete":
            directory = Path(options["state_directory"]) / params["session_id"]
            if directory.exists():
                (directory / "counter.json").unlink(missing_ok=True)
                directory.rmdir()
        send({"id": frame["id"], "result": None})
    elif method == "shutdown":
        send({"id": frame["id"], "result": None})
        break
    elif method:
        send({"id": frame["id"], "result": None})
    else:
        run = pending.pop(frame["id"])
        params = run["params"]
        session = params["session_id"]
        sessions[session] += 1
        (Path(options["state_directory"]) / session / "counter.json").write_text(json.dumps(sessions[session]))
        send({"id": run["id"], "result": {"result": {
            "pid": os.getpid(), "session": session, "count": sessions[session],
            "defaults": options["configuration"], "configuration": params["input"]["configuration"],
        }}})
