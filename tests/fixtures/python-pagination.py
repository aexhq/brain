import os

from brain import Brain

session = Brain(os.environ["BRAIN_TEST_URL"], "fixture-operator").session(os.environ["BRAIN_TEST_SESSION"])
assert sum(1 for _ in session.all_documents("memory")) == 64
assert sum(1 for _ in session.all_interactions()) == 132
assert sum(1 for _ in session.all_files()) == 3
assert sum(1 for _ in session.all_connections()) == 3
assert sum(1 for _ in session.all_schedules()) == 3
counts = {"memory": 0, "interactions": 0, "files": 0, "connections": 0, "schedules": 0}
sections = set()
last = None
for page in session.export_pages():
    sections.add(page["section"])
    for key in counts:
        counts[key] += len(page[key])
    last = page
assert counts == {"memory": 64, "interactions": 132, "files": 3, "connections": 3, "schedules": 3}, counts
assert sections == {"history", "runs", "operations", "interactions", "connections", "files", "state", "memory", "schedules"}, sections
assert last["complete"] and not last["continuation"], last
print("python-pagination: passed")
