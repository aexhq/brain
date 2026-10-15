# Brain for Python

Start durable sessions, read their history, and interact with application work
through Brain's language-neutral Session API v2. The client has no cloud SDK
dependency and does not retry mutations or model effects.

Python 3.12 or newer is required. Install an immutable `aex_brain` wheel from the
matching Brain release. Registry publication is not implied by this source package.

```python
from brain import Brain

brain = Brain(token="YOUR_PARTICIPANT_OR_SERVER_TOKEN")
session = brain.session("YOUR_SESSION_ID")
receipt = session.send("Continue the conversation")
for event in session.events():
    if event["kind"] == "record" and event["record"]["kind"] == "run.completed":
        print(event["record"]["data"]["result"])
        break
```

Session creation takes the generated `brain.generated.SessionConfig`; select an
installed harness and configured model, explicit limits and the capability
catalogue your session needs. Wire types and schemas are generated from Brain's
canonical Rust contracts, including string record sequences and absent usage
counters.

`Session.work()` requires a receipt store. `SQLiteReceipts` durably records the
application's claim before calling its handler and its receipt before posting it.
A crash with a claim but no receipt reports uncertainty, never re-executes the
handler. This does not make arbitrary external business effects exactly once:
applications still need transactional business receipts or explicit reconciliation.
Keep this database private; it contains invocation-scoped receipt credentials.

Use `all_runs()`, `all_operations()`, `all_interactions()`, `all_connections()`,
`all_documents(scope)`, `all_files()` and `all_schedules()` to traverse collections.
Pages have count and byte bounds, so a short page does not mean the end.
`export_pages()` follows all ten export sections, including supplemental usage evidence,
and reports history completeness.
This traversal is not a snapshot or a backup of file bytes, custody and native state.

If a mutation acknowledgement is lost, its uncertain `SessionError.request_id`
retains the original ID, including an SDK-generated one. Explicitly resubmit the
unchanged content and that ID to reconcile; a new ID starts new work.

Trusted operators or original model receipt holders can use `report_usage()` to fill
absent counters on known settled model operations. `usage_evidence()` reads the
bounded, attributed facts. Existing counters and original outcomes cannot change;
unknown effects keep their holds. See
[sparse usage settlement](https://aex.dev/brain/docs/reference/session-history#complete-sparse-usage-without-repeating-work).
