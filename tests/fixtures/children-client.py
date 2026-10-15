import os

from brain import Brain


client = Brain(os.environ["BRAIN_TEST_URL"], token=os.environ.get("BRAIN_TEST_TOKEN", "fixture-operator"))
session = client.session(os.environ["BRAIN_TEST_SESSION"])
result = session.children(os.environ["BRAIN_TEST_RUN"])
assert result["ready"] is True, result
assert [child["status"] for child in result["children"]] == os.environ["BRAIN_EXPECTED_CHILD_STATUSES"].split(",")
assert sum(child["availability"] == "inline" for child in result["children"]) == int(os.environ["BRAIN_EXPECTED_INLINE"])
for child in result["children"]:
    if child["status"] != "completed":
        continue
    reference = child["completion"]
    record = session.child_result(os.environ["BRAIN_TEST_RUN"], child["index"])
    assert record["sequence"] == reference["sequence"]
    assert record["run_id"] == child["run_id"]
    if child["availability"] == "inline":
        assert record["data"]["result"] == child["result"]
    else:
        assert child["availability"] == "referenced" and "result" not in child
print("Python SDK preserved original child outcomes and bounded completion references.")
