"""Independent language fixture for Brain's startup harness protocol."""
import json
import sys

mode = sys.argv[1]
run = None
callback = 0


def send(value):
    print(json.dumps({"jsonrpc": "2.0", **value}), flush=True)


def model():
    global callback
    callback += 1
    send({"id": f"callback:{callback}", "method": "brain/call", "params": {
        "session_id": run["params"]["session_id"], "sequence": run["params"]["sequence"],
        "method": "model", "input": {"messages": [{"role": "user", "content": [{"type": "text", "text": "test"}]}]},
    }})


for line in sys.stdin:
    frame = json.loads(line)
    method = frame.get("method")
    if method == "initialize":
        send({"id": frame["id"], "result": {"protocol": "brain/harness/v1", "name": "python-fixture", "state_version": "1"}})
    elif method == "session/run":
        run = frame
        if mode == "exit":
            sys.exit(0)
        if mode != "cancel":
            model()
        else:
            send({"id": "running", "method": "brain/call", "params": {
                "session_id": run["params"]["session_id"], "sequence": run["params"]["sequence"],
                "method": "emit", "input": {"event_type": "running", "data": {}},
            }})
    elif method == "session/cancel":
        send({"id": frame["id"], "result": None})
        send({"id": run["id"], "result": {}})
    elif method == "shutdown":
        send({"id": frame["id"], "result": None})
        break
    elif method:
        send({"id": frame["id"], "result": None})
    elif mode == "cancel":
        pass
    elif mode == "swallow" and callback == 1:
        model()
    else:
        send({"id": run["id"], "result": {"result": {"ok": True}}})
