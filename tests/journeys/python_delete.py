import json
import os
from brain import Brain, BrainError

client = Brain(os.environ["JOURNEY_URL"], os.environ["JOURNEY_A"])
session = client.create({"harness": "pi", "config": {"model": "openai/gpt-4.1-mini", "tools": []}})
client.delete(session["id"])
client.delete(session["id"])
try:
    client.get(session["id"])
except BrainError as error:
    assert error.status == 404
else:
    raise AssertionError("deleted session remained readable")
print(json.dumps({"deleted": True}))
