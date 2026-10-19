# Attach a browser tab through your gateway

Keep Brain management and model credentials in your backend. Serve your app and the gateway from one origin. Your platform authenticates its user, chooses a public session composition and calls `gateway.issue()` in trusted server code. There is no public grant-issuance route.

```javascript
import { browserGateway } from './gateway.mjs';

const gateway = browserGateway({
  origin: 'https://app.example.com',
  brainURL: 'http://brain.internal:8080',
  brainToken: process.env.BRAIN_APP_TOKEN,
});
const readTab = {
  name: 'read_tab', description: 'Read the attached application page',
  input_schema: { type: 'object', properties: {}, additionalProperties: false },
};

// After your platform authorizes this user and composition:
const grant = gateway.issue({
  create: { harness: 'pi', config: { model: 'openai/gpt-4.1-mini', tools: [] } },
  tools: [readTab], ttlMs: 300000,
});
// Deliver grant.token to that user's page through your authenticated app route.
// Route /composition, /sessions/* and /executors/* to gateway.handle(req, res).
```

Bundle the installed TypeScript SDK in your page. Call `connectTab` directly in a click handler:

```javascript
import { Brain, connectTab } from '@aexhq/brain';

const brain = new Brain(location.origin, pageGrant.token);
const composition = await (await fetch('/composition', {
  headers: { Authorization: `Bearer ${pageGrant.token}` },
})).json();
let attachment;
document.querySelector('#attach').addEventListener('click', async () => {
  attachment = await connectTab(brain, {
    create: { ...composition.create, runtime_tools: [{
      ...composition.tools[0],
      run() { return { text: document.querySelector('#app').innerText }; },
    }] },
  });
  await brain.send(attachment.session.id, { type: 'user.message', text: 'Read my attached page' });
});
```

The page declares the function; it can access only its own browser execution context and the permissions the browser gives it. Navigation or closing the page ends its executor connection. Calls already dispatched can have unknown outcomes; the gateway and SDK never replay them. Keep a dedicated `Brain` client for that attached page. `attachment.detach()` ends that client's runtime connections explicitly.

The gateway approves exactly the configured tool descriptors and session composition. Its page grant can create one session or operate a specified existing session; it cannot list other sessions or use management routes. Executor credentials returned to the page are separate opaque gateway capabilities. Backend Brain credentials remain in memory on the gateway. Requests must come from the configured origin; expiry and `await gateway.revoke(grant.token)` abort open streams and revoke executors. Grants expire after five minutes by default and accept at most fifteen minutes.

For reattachment, authorize the existing session in your platform, issue `{ session: id, tools: [readTab] }`, and call `connectTab(brain, { session: id, tools: [runtimeFunction] })` from a new gesture. Only future calls use the new executor. Your platform owns login, grant delivery, retention and cleanup of sessions created before an ambiguous gateway failure.

Run `node scripts/setup.mjs` and `node scripts/check.mjs`. The real Chromium [journeys](../../tests/journeys/browser.test.mjs) exercise attachment, event streaming, tool calls, duplicate replies, closed tabs, denied origins and compositions, expiry, revocation, late results and explicit rebind.
