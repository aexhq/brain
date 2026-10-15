import json
import os
from contextlib import closing
from uuid import uuid4

from brain import Brain

client = Brain(os.environ["BRAIN_TEST_URL"], "fixture-operator", timeout=10)
session = client.create(json.loads(os.environ["BRAIN_TEST_CONFIG"]), "Python first turn")
with closing(session.events()) as events:
    first = next(item for item in events if item["kind"] == "record" and item["record"]["kind"] == "run.completed")
assert session.read()["lifecycle"] == "open"
request_id = str(uuid4())
accepted = session.send("Python continuation", request_id)
assert session.send("Python continuation", request_id)["id"] == accepted["id"]
with closing(session.events(first["id"])) as events:
    second = next(item for item in events if item["kind"] == "record" and item["record"]["kind"] == "run.completed")
assert int(second["id"]) > int(first["id"])
assert len(session.runs()) == 2
session.put_document("memory", "language", {"request_id": str(uuid4()), "revision": "0", "value": "python"})
assert session.documents("memory")[0]["value"] == "python"
session.put_file("artifact.txt", b"Python artifact", "text/plain")
assert session.file("artifact.txt") == b"Python artifact"
assert session.files()[0]["provenance"] == "operator"
assert session.export()["session"]["id"] == session.id
session.delete_file("artifact.txt")
session.delete_document("memory", "language")
assert session.files() == []
assert session.documents("memory") == []
session.control("close")
assert session.read()["lifecycle"] == "closed"
print("python-public-journey: passed")
