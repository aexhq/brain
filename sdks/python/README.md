# Brain Python SDK

Install with `python -m pip install aex-brain==2.0.0`. Requires Python 3.11+;
there are no runtime dependencies. Import the client from `brain` and generated
`TypedDict` definitions from `brain.types`. The wheel includes PEP 561 type data.

```python
import os
from contextlib import closing
from brain import Brain, Model

brain = Brain('http://127.0.0.1:8080', os.environ['BRAIN_API_KEY'])
session = brain.create(harness='pi', model=Model('openai/gpt-4.1-mini'), tools=[])
message = brain.send(session['id'], {'type': 'user.message', 'text': 'Hello!'})
with closing(brain.observe(session['id'])) as events:
    for event in events:
        if event['type'] == 'assistant.message':
            print(event['data']['text'])
        if event.get('input_id') != message['id']:
            continue
        if event['type'] == 'session.error':
            raise RuntimeError(event['data']['message'])
        if event['type'] == 'session.status' and event['data']['status'] == 'idle':
            break
```

This is a synchronous client. `timeout=30` configures the socket timeout, including
stream reads. Requests are never automatically retried. `BrainError` preserves the
HTTP status, server error code, and request ID. Closing the observer does not stop
execution; call `interrupt(session_id)` for that.

Pass `tools=[Tool(...)]` to `create()` to run Python functions in your client. Select installed tools with `ManagedTool(name)`. Supply private model credentials with `Model(name, ApiKey(key))` or `CredentialReference(reference)`; the server needs independent encryption keys.

The SDK owns the SSE/HTTP connection. Keep the process alive, honor
`context.cancelled`, and call `dispose()` on shutdown. Use `rebind()` after recovery;
callbacks are never automatically replayed.

Pin the package and matching server image commit recorded by the release workflow.
[SDK reference](https://aexhq.github.io/brain/docs/sdks/python/) ·
[Server setup](https://aexhq.github.io/brain/docs/quickstart/)
