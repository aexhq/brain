import os

from brain import Brain


client = Brain(os.environ["BRAIN_TEST_URL"], token="fixture-operator")
budget = client.session(os.environ["BRAIN_TEST_SESSION"]).budget()
assert budget["calls"] == 1, budget
assert budget["reported_tokens"] == "7", budget
assert budget["reserved_tokens"] == os.environ["BRAIN_EXPECTED_HELD"], budget
assert budget["incomplete_usages"] == 1, budget
assert budget["token_usage_pending"] is True, budget
print("Python SDK retained the original incomplete-usage reservation.")
