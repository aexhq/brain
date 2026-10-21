# Brain server for Node.js

Install Brain, start a local server, and run sessions from your application:

```sh
npm install brainserver
```

Use Node.js 24 or newer. Set `OPENAI_API_KEY`, save this as `agent.mjs`, and run `node agent.mjs`:

```js
import { BrainServer, Model } from 'brainserver';

const brain = await BrainServer.start();
try {
  const session = await brain.create({
    harness: 'pi', model: Model('openai/gpt-4.1-mini'), tools: [],
  });
  const input = await brain.send(session.id, { type: 'user.message', text: 'Say hello.' });
  for await (const event of brain.observe(session.id)) {
    if (event.type === 'assistant.message') console.log(event.data.text);
    if (event.input_id !== input.id) continue;
    if (event.type === 'session.error') throw new Error(event.data.message);
    if (event.type === 'session.status' && event.data.status === 'idle') break;
  }
} finally {
  await brain.stop();
}
```

The package includes the compiled Pi harness, its pinned dependencies, the client,
and a prebuilt Go executable selected by npm for your platform. Users need Node,
with no Go toolchain or separate build. Installation works with `--ignore-scripts`.
Linux, macOS, and Windows support x64 and arm64.

`BrainServer.start()` waits for startup, binds to an available loopback port, creates
a random local API token, and saves session data in `.brain/` in your working directory.
The returned object has the `Brain` client's methods and its `url`. Call `stop()` to
drain the server and its harnesses; closing the parent process also closes the
server's lifetime pipe and triggers shutdown. `exited` resolves with the exit status.
Callers that need to monitor an unexpected server exit can await it.

Configure the data directory, TCP port, or providers in your script:

```js
const brain = await BrainServer.start({
  dataDir: './agent-data',
  port: 8080,
  providers: {
    openai: { apiKey: process.env.OPENAI_API_KEY },
    anthropic: { apiKey: process.env.ANTHROPIC_API_KEY },
  },
});
```

Provider entries accept `apiKey`, an optional `baseUrl`, and an optional `api`
transport name. The server validates provider configuration during startup.
Keys are passed in the child environment, never written into the temporary host
configuration. The API token belongs to the `local` tenant and can manage sessions.
For application functions, use the exported `Tool` constructor in `brain.create()`.
For a separately deployed server, install `@aexhq/brain` and connect with `Brain`.

## Build from this repository

After `node scripts/setup.mjs`, run:

```sh
node scripts/package-server.mjs
node --test tests/server-package.test.mjs
```

The packaging command builds the client, Pi harness, and executable together.
It writes npm archives and `release.json` to `dist/server/`. Add `--all-platforms`
to cross-compile every supported executable. Install your platform archive and
the main archive together when testing an unpublished build. CI tests the
installed archives outside the checkout before publishing those same files.
