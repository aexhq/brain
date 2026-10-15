import json
import os

from brain import Brain, SessionError

session = Brain(os.environ["BRAIN_URL"], "fixture-operator").session(os.environ["BRAIN_SESSION"])
receipt = json.loads(os.environ["BRAIN_RECEIPT"])
operation = session.receipt(os.environ["BRAIN_OPERATION"], receipt, os.environ["BRAIN_RECEIPT_TOKEN"])
assert operation["status"] == "completed"
assert operation["result_available"] is False
assert "result" not in operation
assert operation["usage"]["input_tokens"] == 9
assert "output_tokens" not in operation["usage"]
assert operation["usage"]["provider"] == {}
assert len(operation["usage"]["provider_unavailable"]["digest"]) == 64
try:
    session.read()
except SessionError as error:
    assert error.status == 404
else:
    raise AssertionError("original receipt restored the deleted session")
print("Original deleted receipt remains evidence without bodies")
