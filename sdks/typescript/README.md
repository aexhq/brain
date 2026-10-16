# Brain JavaScript and TypeScript SDK

Install with `npm install --save-exact @aexhq/brain@1.0.0`. Requires Node.js 24+;
there are no runtime dependencies. JavaScript and TypeScript use the same package.

```js
import { Brain } from '@aexhq/brain';

const brain = new Brain('http://127.0.0.1:8080', process.env.BRAIN_API_KEY);
const session = await brain.create({
  harness: 'pi', config: { model: 'openai/gpt-4.1-mini', tools: [] },
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

Version 1 replaces the predecessor's 0.x SDK and requires the current Go session
server. It does not implement the old environment/worker APIs. Pin the SDK version
and the matching server image commit recorded by the release workflow.

[SDK reference](https://aexhq.github.io/brain/docs/sdks/typescript/) ·
[Server setup](https://aexhq.github.io/brain/docs/quickstart/)
