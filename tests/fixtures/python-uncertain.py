import os

from brain import Brain, SessionError

session = Brain(os.environ["BRAIN_TEST_URL"], "fixture-operator").session(os.environ["BRAIN_TEST_SESSION"])
try:
    session.send("python uncertain acknowledgement")
    raise AssertionError("the fixture did not lose the acknowledgement")
except SessionError as error:
    assert error.uncertain, error.code
    assert error.request_id and len(error.request_id) == 36
    original_id = error.request_id
receipt = session.send("python uncertain acknowledgement", request_id=original_id)
assert receipt["id"]
print("python-uncertain-reconciliation: passed")
