"""Invoked by the local journey suite with a running Brain and recording provider."""
import json
import os
import time

from brain import Brain, BrainError


def rejected(action, status, code):
    try:
        action()
    except BrainError as error:
        assert (error.status, error.code) == (status, code), vars(error)
        assert error.request_id
    else:
        raise AssertionError("request unexpectedly succeeded")


def finished(client, session, input_id):
    stream = client.observe(session)
    seen = []
    try:
        for event in stream:
            seen.append(event)
            assert not (event["type"] == "session.error" and event.get("input_id") == input_id), event
            if event["type"] == "session.status" and event.get("input_id") == input_id and event["data"]["status"] == "idle":
                return seen
    finally:
        stream.close()
    raise AssertionError("stream ended before completion")


url, token = os.environ["JOURNEY_URL"], os.environ["JOURNEY_A"]
client = Brain(url, token, timeout=15)
session = os.environ["JOURNEY_SESSION"]
assert client.get(session)["config"]["tools"] == ["echo", "ask_approval"]
assert "echo" in [tool["name"] for tool in client.catalog()["tools"]]
assert session in [s["id"] for s in client.list()["sessions"]]
rejected(lambda: Brain(url, os.environ["JOURNEY_B"]).get(session), 404, "not_found")
rejected(lambda: Brain(url, "invalid").list(), 401, "forbidden")

payload = {"type": "user.message", "text": 'CALL echo {"text":"Python says hello 世界"}', "key": "python"}
sent = client.send(session, payload)
assert client.send(session, payload)["id"] == sent["id"]
events = finished(client, session, sent["id"])
assert any(e["type"] == "tool.result" and "Python says hello 世界" in json.dumps(e["data"], ensure_ascii=False) for e in events)
assert client.history(session, limit=1)["events"][0]["type"] == "user.message"
assert client.history(session, after=events[-1]["cursor"])["events"] == []

checkpoint = client.flush(session)
assert checkpoint["native_checkpoint"] == client.sync(session)["native_checkpoint"]
assert client.restore(session, checkpoint["checkpoint"])["status"] == "interrupted"
rejected(lambda: client.send(session, {"type": "user.message", "text": "resume required"}), 409, "conflict")
assert client.resume(session)["status"] == "idle"

pending = client.send(session, {"type": "user.message", "text": 'CALL ask_approval {"question":"Python cancels this wait"}'})
stream = client.observe(session)
try:
    for event in stream:
        if event["type"] == "tool.progress" and event.get("input_id") == pending["id"]:
            break
finally:
    stream.close()
client.interrupt(session)
deadline = time.monotonic() + 10
while client.get(session)["status"] != "interrupted":
    assert time.monotonic() < deadline, "interrupt did not settle"
    time.sleep(0.02)
assert client.resume(session)["status"] == "idle"
continued = client.send(session, {"type": "user.message", "text": "Python resumed"})
finished(client, session, continued["id"])
client.close(session)
rejected(lambda: client.resume(session), 409, "conflict")

created = client.create({"harness": "pi", "config": {"model": "openai/gpt-4.1-mini", "reasoning_effort": "off", "tools": []}})
sent = client.send(created["id"], {"type": "user.message", "text": "Python created this session"})
finished(client, created["id"], sent["id"])
client.close(created["id"])
print(json.dumps({"created": created["id"]}))
