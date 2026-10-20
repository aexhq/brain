# Brain JavaScript and TypeScript SDK

Install with `npm install --save-exact @aexhq/brain@2.1.0`. Requires Node.js 24+;
there are no runtime dependencies. JavaScript and TypeScript use the same package.

```js
import { Brain, Model } from '@aexhq/brain';

const brain = new Brain('http://127.0.0.1:8080', process.env.BRAIN_API_KEY);
const session = await brain.create({
  harness: 'pi', model: Model('openai/gpt-4.1-mini'), tools: [],
});
const input = await brain.send(session.id, { type: 'user.message', text: 'Hello!' });
for await (const event of brain.observe(session.id)) {
  if (event.type === 'assistant.message') console.log(event.data.text);
  if (event.input_id !== input.id) continue;
  if (event.type === 'session.error') throw new Error(event.data.message);
  if (event.type === 'session.status' && event.data.status === 'idle') break;
}
```

Tokens belong in a trusted backend. Requests are never automatically retried.
`BrainError` exposes HTTP status, error code, and request ID. Stopping observation
closes the stream; call `interrupt(id)` to interrupt execution. Reconnect with
`observe(id, { after: cursor, signal })` using the last retained event cursor.

Pass `Tool({ name, description, input_schema, run })` in `tools` to run functions in your client. Select installed tools with `ManagedTool(name)`. Supply private model credentials with `Model(name, ApiKey(key))` or `CredentialReference(reference)`; the server needs independent encryption keys.

The SDK owns SSE/HTTP or WebSocket callback connections; function source and captured
credentials stay local. Keep the process alive and call `dispose()` on shutdown.
After recovery, `rebind()` attaches matching functions without replaying actions.

Version 2 uses session bindings and structured result schemas. Update the server,
runner, and client together, and pin the matching release commit.

[SDK reference](https://aexhq.github.io/brain/docs/sdks/typescript/) ·
[Server setup](https://aexhq.github.io/brain/docs/quickstart/)
