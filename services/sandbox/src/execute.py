"""Runs inside the remote Linux sandbox, never on the tool service host."""
import base64
import json
import os
import queue
import signal
import subprocess
import sys
import threading
import time


def execute(command, timeout=30, limit=65536):
    proc = subprocess.Popen(command, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            start_new_session=os.name == "posix")
    chunks = queue.Queue(maxsize=16)

    def read(stream, name):
        while data := stream.read1(4096):
            chunks.put((name, data))
        chunks.put((name, None))

    for name in ("stdout", "stderr"):
        threading.Thread(target=read, args=(getattr(proc, name), name), daemon=True).start()
    output = {"stdout": bytearray(), "stderr": bytearray()}
    size, ended = 0, 0
    deadline = time.monotonic() + timeout
    timed_out = truncated = False
    try:
        while ended < 2:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                timed_out = True
                break
            try:
                name, data = chunks.get(timeout=remaining)
            except queue.Empty:
                timed_out = True
                break
            if data is None:
                ended += 1
                continue
            output[name].extend(data[:max(0, limit - size)])
            size += len(data)
            if size > limit:
                truncated = True
                break
    finally:
        if timed_out or truncated:
            try:
                if os.name == "posix":
                    os.killpg(proc.pid, signal.SIGKILL)
                else:
                    proc.kill()
            except ProcessLookupError:
                pass
        proc.wait()
    return {**{k: v.decode("utf-8", errors="replace") for k, v in output.items()},
            "exit_code": proc.returncode, "timed_out": timed_out, "truncated": truncated}


if __name__ == "__main__":
    print(json.dumps(execute(base64.b64decode(sys.argv[1]).decode("utf-8"))))
