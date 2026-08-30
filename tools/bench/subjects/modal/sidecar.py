"""Modal's sandbox lifecycle, exposed to the benchmark runner as a line protocol.

Modal publishes no HTTP API for sandboxes. Every SDK — Python, JavaScript, Go — speaks
protobuf over gRPC to a control plane their own documentation calls "not a public API",
so there is nothing for the runner's HTTP client to talk to. This subject is therefore
the one measured through a vendor SDK rather than through `reqwest` like every other
hosted subject, and its manifest says so: the Python interpreter and Modal's gRPC stack
are inside every Modal number, where the others carry the runner's own client.

One JSON request per line on stdin, one JSON response per line on stdout. The process is
long-lived so that neither the interpreter's start-up nor the SDK's import cost — the
better part of a second each — can land inside a measurement. Anything that fails comes
back as `{"ok": false, "error": ...}` carrying the traceback, because a driver that can
only say "it failed" makes every failure look identical.
"""

import json
import sys
import traceback

import modal

# Fixed and trivial, because what is being measured is the sandbox's cost of running
# something, not the something. `sleep infinity` keeps the sandbox alive between execs.
IDLE = ("sleep", "infinity")
COMMAND = ("echo", "benchmark")
EXPECTED = "benchmark"


def main() -> None:
    # The SDK is not supposed to write to stdout, and if it ever does the line lands in
    # the middle of a response and desynchronises the protocol for the rest of the run.
    # Handing it stderr instead costs nothing and removes the failure mode.
    protocol = sys.stdout
    sys.stdout = sys.stderr

    try:
        # Neither of these is per-sandbox work, which is why the driver does this in
        # `prepare` and before the ready line: front-loading differs per subject and is
        # never timed.
        app = modal.App.lookup("brain-bench", create_if_missing=True)
        image = modal.Image.debian_slim()
    except Exception:
        reply(protocol, {"ok": False, "error": traceback.format_exc()})
        return
    reply(protocol, {"ok": True, "sdk": modal.__version__, "app": app.app_id})

    sandboxes: dict[str, modal.Sandbox] = {}
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            answer = handle(json.loads(line), sandboxes, app, image)
        except Exception:
            answer = {"ok": False, "error": traceback.format_exc()}
        reply(protocol, answer)


def handle(request: dict, sandboxes: dict, app, image) -> dict:
    op = request.get("op")
    if op == "ping":
        return {"ok": True}

    if op == "create":
        # Modal's documented defaults for CPU, memory and timeout, with their standard
        # base image: a rival measured at settings we chose is not the rival anyone else
        # runs, and the number would be worth nothing.
        sandbox = modal.Sandbox.create(*IDLE, app=app, image=image)
        sandboxes[sandbox.object_id] = sandbox
        return {"ok": True, "id": sandbox.object_id}

    identifier = request.get("id")
    sandbox = sandboxes.get(identifier)
    if sandbox is None:
        return {"ok": False, "error": f"no sandbox {identifier!r} was created by this sidecar"}

    if op == "ttfb":
        process = sandbox.exec(*COMMAND)
        first = next(iter(process.stdout), "")
        # A first chunk that is not the command's output means the exec did not run, and
        # the time to it is not a time-to-first-byte for anything.
        if EXPECTED not in first:
            return {
                "ok": False,
                "error": f"the first stdout chunk was {first!r}, not the command's output",
            }
        return {"ok": True}

    if op == "round_trip":
        process = sandbox.exec(*COMMAND)
        code = process.wait()
        # A non-zero exit means the command did not run, whatever the SDK returned.
        if code != 0:
            return {"ok": False, "error": f"the command exited {code}"}
        return {"ok": True}

    if op == "destroy":
        # `wait=False`, and the driver's doc comment says why: Modal takes a measured ~31
        # seconds to actually release a sandbox, which would put thirteen minutes of
        # teardown inside a 25-sample create probe. Nothing downstream depends on the
        # release having completed, because a hosted subject exposes no process for the
        # memory sampler to read and its reclaim probe is refused outright.
        sandbox.terminate()
        sandboxes.pop(identifier, None)
        return {"ok": True}

    return {"ok": False, "error": f"unknown op {op!r}"}


def reply(protocol, payload: dict) -> None:
    protocol.write(json.dumps(payload) + "\n")
    protocol.flush()


if __name__ == "__main__":
    main()
