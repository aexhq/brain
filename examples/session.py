"""Run with PYTHONPATH=sdks/python and BRAIN_API_KEY set."""
import os
import sys
import uuid

from brain import Brain

brain = Brain(os.environ.get("BRAIN_URL", "http://127.0.0.1:8080"), os.environ["BRAIN_API_KEY"])
session = brain.create({"harness": "pi", "config": {"model": os.environ.get("BRAIN_MODEL", "openai/gpt-4.1-mini"), "tools": []}})
print("Session:", session["id"])
message = brain.send(session["id"], {"type": "user.message", "text": " ".join(sys.argv[1:]) or "Say hello.", "key": str(uuid.uuid4())})
stream = brain.observe(session["id"])
try:
    for event in stream:
        if event["type"] == "assistant.message":
            print(event["data"]["text"])
        if event.get("input_id") == message["id"]:
            if event["type"] == "session.error":
                raise RuntimeError(event["data"]["message"])
            if event["type"] == "session.status" and event["data"]["status"] == "idle":
                break
finally:
    stream.close()
