"""Installed Python SDK callback lifecycle against real Brain and Pi."""
import json
import os

from brain import Brain, RuntimeTool

client = Brain(os.environ["JOURNEY_URL"], os.environ["JOURNEY_A"])
calls = []

def run(arguments, context):
    calls.append(context.scope)
    assert context.scope["tenant_id"] == "a"
    assert context.scope["session_id"] != arguments.get("session_id")
    assert not context.cancelled.is_set()
    context.progress({"stage": "read"})
    return {"answer": "Python callback"}

tool = RuntimeTool("lookup", "Read private application data", {"type": "object"}, run,
                   {"type": "object", "required": ["answer"]})
session = client.create({"harness": "pi", "config": {"model": "openai/gpt-4.1-mini", "tools": []}}, runtime_tools=[tool])
assert [t["name"] for t in session["descriptors"]] == ["runtime_lookup"]
sent = client.send(session["id"], {"type": "user.message", "text": 'CALL runtime_lookup {"session_id":"spoof"}'})
stream = client.observe(session["id"])
try:
    for event in stream:
        assert event["type"] != "session.error", event
        if event["type"] == "session.status" and event.get("input_id") == sent["id"] and event["data"]["status"] == "idle":
            break
finally:
    stream.close()
events = client.history(session["id"])["events"]
result = next(e["data"]["result"] for e in events if e["type"] == "tool.result")
assert result["structured_content"] == {"answer": "Python callback"}, result
assert len(calls) == 1
client.close(session["id"])
client.dispose()
print(json.dumps({"session": session["id"], "calls": len(calls)}))
