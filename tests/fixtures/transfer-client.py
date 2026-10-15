import os

from brain import Brain, SessionError

client = Brain(os.environ["BRAIN_URL"], os.environ["BRAIN_TOKEN"])
session = client.session(os.environ["BRAIN_SESSION"])
transfer = session.ownership_transfer(os.environ["BRAIN_TRANSFER"])
assert transfer["phase"] == "transferred"
assert any(node["id"] == transfer["target_node"] for node in client.nodes())
replay = session.transfer_ownership({
    "request_id": "planned-move",
    "owner_epoch": transfer["source_epoch"],
    "target_node": transfer["target_node"],
})
assert replay["id"] == transfer["id"]
assert replay["phase"] == "transferred"
try:
    session.cancel_transfer(transfer["id"])
except SessionError as error:
    assert error.code == "conflict"
else:
    raise AssertionError("completed ownership was reversed")
assert session.records()["gaps"] == []
