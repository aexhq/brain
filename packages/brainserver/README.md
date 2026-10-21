# Brain server for Node.js

Install Brain, start a local server, and run sessions from your application:

```sh
npm install brainserver
```

Use Node.js 24 or newer. Set `OPENAI_API_KEY`, save this as `agent.mjs`, and run `node agent.mjs`:

```js
import { BrainServer, Model } from 'brainserver';

const brain = new BrainServer();
await brain.start();
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

`new BrainServer(options)` configures your application. `brain.start()` starts it,
binds to an available loopback port, and saves session data in `.brain/` in your working directory.
The instance has the `Brain` client's methods and its `url`. Call `stop()` to
drain the server and its harnesses; closing the parent process also closes the
server's lifetime pipe and triggers shutdown. `exited` resolves with the exit status.
Callers that need to monitor an unexpected server exit can await it.

Configure the server and install integrations in its constructor:

```js
import { BrainServer, Modal, AwsS3 } from 'brainserver';

const server = new BrainServer({
  apiKey: process.env.BRAIN_API_KEY,
  dataDir: './agent-data',
  port: 8080,
  providers: {
    openai: { apiKey: process.env.OPENAI_API_KEY },
  },
  tools: [
    Modal({
      tokenId: process.env.MODAL_TOKEN_ID,
      tokenSecret: process.env.MODAL_TOKEN_SECRET,
    }),
    AwsS3({
      bucket: 'documents', region: 'eu-west-2',
      accessKeyId: process.env.AWS_ACCESS_KEY_ID,
      secretAccessKey: process.env.AWS_SECRET_ACCESS_KEY,
    }),
  ],
});
await server.start();
```

Set `BRAIN_API_KEY` to a token of at least 24 bytes to connect from another process.
Without an `apiKey` option, the instance generates a token for its own client methods.
Model provider entries accept optional `apiKey`, `baseUrl`, and `api` settings.

## Select tools and credentials in a session

The same helpers are exported by `@aexhq/brain`. A session selects installed
integrations and can supply private credentials:

```js
import { Brain, Model, Modal, AwsS3 } from '@aexhq/brain';

const brain = new Brain('http://127.0.0.1:8080', process.env.BRAIN_API_KEY);
const session = await brain.create({
  harness: 'pi', model: Model('openai/gpt-4.1-mini'),
  tools: [
    Modal(), // Uses the server's default credentials.
    AwsS3({
      bucket: 'customer-documents', region: 'eu-west-2', operations: ['read'],
      accessKeyId: customerAccessKeyId,
      secretAccessKey: customerSecretAccessKey,
    }),
  ],
});
```

`Modal()`, `AwsS3()`, `E2B()`, and `Daytona()` can be installed without credentials.
An explicit session credential takes precedence over the server default. Omit it
to use the default. Selecting an integration without either source fails before
inference. Malformed, revoked, or rejected overrides fail without selecting a
server default. Wrong cloud credentials produce tool errors when the provider rejects them.

S3 offers `list` and `read` by default. Enable `write` and `delete` by including them
in `AwsS3({ operations: [...] })` on the server; sessions select a subset through
the same option. Sandbox helpers offer `create`, `exec`, `inspect`, and `terminate`.
The model sees names such as `aws_s3_read` and `modal_exec`. No integration downloads
or cloud actions occur when selecting a session's tools.

Resource settings can be supplied at registration or session creation. Changing
a resource fixed by the server requires private session credentials. Custom
service endpoints remain server-owned. S3 operations retain tenant-scoped keys
and the existing text size limits; sandbox handles retain tenant/session ownership
and expiry. See [resource tools](../../services/resources/README.md) and
[sandbox providers](../../services/sandbox/README.md) for execution behavior.

Session credentials are encrypted before storage and kept out of public config,
events, and model arguments. The local encryption key is `<dataDir>/credential.key`;
checkpoints omit it. Keep that key when moving or restoring private-credential sessions.
Server defaults remain in the server environment and are not recorded as session overrides.
The API token belongs to the `local` tenant and can manage sessions.
For application functions, use the exported `Tool` constructor in `brain.create()`.
For a separately deployed server, install `@aexhq/brain` and connect with `Brain`.

## Build from this repository

After `node scripts/setup.mjs`, run:

```sh
npm run build
node --test tests/server-package.test.mjs
```

Run the build from the repository root. It builds the client, Pi harness, and executable together.
It writes npm archives and `release.json` to `dist/server/`. Add `-- --all-platforms`
to cross-compile every supported executable. Install your platform archive and
the main archive together when testing an unpublished build. CI tests the
installed archives outside the checkout before publishing those same files.
