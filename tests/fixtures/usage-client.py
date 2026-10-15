import json
import os

from brain import Brain, SessionError

session = Brain(os.environ["BRAIN_URL"], "fixture-operator").session(os.environ["BRAIN_SESSION"])
operation_id = os.environ["BRAIN_OPERATION"]
request = json.loads(os.environ["BRAIN_USAGE"])
expected = json.loads(os.environ["BRAIN_EXPECTED_USAGE"])
token = os.environ.get("BRAIN_RECEIPT_TOKEN")
operation = session.report_usage(operation_id, request, token)
assert operation["status"] == "completed"
for key, value in expected.items():
    assert operation["usage"][key] == value
assert session.report_usage(operation_id, request, token) == operation

if os.environ.get("BRAIN_DELETED") == "1":
    assert not operation["result_available"] and "result" not in operation
    assert operation["usage"]["provider"] == {}
    assert len(operation["usage"]["provider_unavailable"]["digest"]) == 64
    for read in (session.read, lambda: session.usage_evidence(operation_id)):
        try:
            read()
        except SessionError as error:
            assert error.status == 404
        else:
            raise AssertionError("usage evidence restored a deleted session")
else:
    evidence = []
    after = ""
    while page := session.usage_evidence(operation_id, 1, after):
        evidence.extend(page)
        after = page[-1]["id"]
    assert 1 <= len(evidence) <= 3
    assert any(value["request_id"] == request["request_id"] for value in evidence)
    assert all(value["source"] == "operator" and value["subject"] == "operator" for value in evidence)
print("Supplemental usage is durable without replacing original outcomes")
