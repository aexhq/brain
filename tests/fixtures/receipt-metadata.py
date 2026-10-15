import os

from brain import Brain

client = Brain(os.environ["BRAIN_URL"], "fixture-operator")
session = client.session(os.environ["BRAIN_SESSION"])
operation = session.operation(os.environ["BRAIN_OPERATION"])
assert operation["status"] == "completed" and operation["result_available"]
assert len(operation["result"]["payload"]) == 6000
assert operation["usage"]["input_tokens"] == 9
assert "output_tokens" not in operation["usage"]
assert operation["usage"]["provider"] == {}
assert operation["usage"]["provider_unavailable"]["bytes"] == 900014
assert len(operation["usage"]["provider_unavailable"]["digest"]) == 64
assert session.budget()["reserved_output_bytes"] == 0
print("Original completed receipt and bounded metadata remain readable")
