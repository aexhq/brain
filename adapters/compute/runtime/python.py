import asyncio
import importlib.util
import inspect
import json
import sys
from types import MappingProxyType

raw = sys.stdin.buffer.read(262145)
if len(raw) > 262144:
    raise SystemExit(65)
invocation = json.loads(raw)
execution = invocation["execution"]
output = sys.stdout
sys.stdout = sys.stderr
sys.path[:0] = ["/work", "/work/dependencies"]
try:
    spec = importlib.util.spec_from_file_location("brain_user_entry", "/work/" + invocation["entrypoint"])
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    action = getattr(module, "actions", {}).get(execution["operation"]["action"]) or getattr(module, "main", None)
    if not callable(action):
        raise ValueError("action_not_exported")
    result = action(execution["arguments"], MappingProxyType({
        "operation_id": execution["operation"]["id"],
        "session_id": execution["operation"]["session_id"],
        "action": execution["operation"]["action"],
        "deadline_ms": execution["deadline_ms"],
    }))
    if inspect.isawaitable(result):
        result = asyncio.run(result)
    try:
        serialized = json.dumps(result, ensure_ascii=False, allow_nan=False, separators=(",", ":")).encode()
        receipt = {"outcome": "completed", "result": result} if len(serialized) <= execution["max_output_bytes"] else {
            "outcome": "completed", "error": "managed result exceeded its admitted output bound"
        }
    except (TypeError, ValueError, OverflowError):
        receipt = {"outcome": "completed", "error": "managed result was not JSON serializable"}
except BaseException:
    receipt = {"outcome": "failed", "error": "managed action failed; secret-bearing exception details are not exported"}
output.write(json.dumps({"protocol": "brain-managed-result/1", **receipt}, allow_nan=False, separators=(",", ":")) + "\n")
output.flush()
