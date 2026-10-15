import os

from brain import Brain, SessionError


client = Brain(os.environ["BRAIN_URL"], os.environ["BRAIN_TOKEN"])
identity = client.identity()
assert identity["tenant"] == "beta"
assert identity["operator"] is False
assert identity["subject"].startswith("identity:")
assert [session["id"] for session in client.sessions()] == [os.environ["BRAIN_SESSION"]]
try:
    client.session(os.environ["BRAIN_OTHER_SESSION"]).read()
except SessionError as error:
    assert error.status == 404
else:
    raise AssertionError("a signed identity read a different tenant's session")
client.session(os.environ["BRAIN_SESSION"]).send("python identity authorized", "python-identity")
print("python identity isolated")
