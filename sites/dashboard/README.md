# Brain dashboard

Create and inspect sessions, stream responses and tool activity, send messages and attachments, interrupt or resume work, close and delete sessions, inspect the tool catalog, and read provider-reported token usage.

## Run locally

Use Node.js 24+ and a running Brain deployment. From the repository root:

```sh
npm --prefix clients/sdks/typescript ci --ignore-scripts
npm --prefix clients/sdks/typescript run build
npm --prefix sites/dashboard ci --ignore-scripts
npm --prefix sites/dashboard run dev
```

Open `http://127.0.0.1:3100/` and enter a Brain API token. The server connects to `http://127.0.0.1:8080` by default. To use another deployment, set `BRAIN_API_URL` in the dashboard process environment before starting it:

```powershell
$env:BRAIN_API_URL = 'https://brain.example.com'
npm --prefix sites/dashboard run dev
```

```sh
BRAIN_API_URL=https://brain.example.com npm --prefix sites/dashboard run dev
```

The URL must be an HTTP(S) origin without a path, query, or credentials. It is configured on the server, not selected by browser users. Use a token scoped to the tenant and permissions you need. Brain enforces authorization on every proxied request; read-only tokens can inspect sessions but cannot change them.

## Build and serve

```sh
npm --prefix clients/sdks/typescript run build
npm --prefix sites/dashboard run build
npm --prefix sites/dashboard start
```

The dashboard requires a Node.js server; it is not a static export. `start` binds to loopback on port 3100. For a different bind address or port, run the Next CLI directly from `sites/dashboard`, for example `npm exec -- next start --hostname 0.0.0.0 --port 3100`.

For remote access, serve it behind HTTPS. Preserve the browser's `Host` header and disable proxy buffering for `/api/brain/sessions/*/events` so streamed events arrive promptly. Give those streams a suitable long-lived connection timeout. Brain's deployment address must be reachable from the dashboard server.

The connection endpoint verifies the token with Brain and stores it in an HttpOnly, SameSite=Strict cookie for eight hours; HTTPS connections also set Secure. The token is not returned to client JavaScript or written to local storage. Mutations require a matching Origin header. The proxy permits only the session, event, attachment, and catalog routes used by the dashboard and never accepts a destination URL from the browser. Disconnect clears the cookie; revoke the token at Brain to revoke its API access.

Model credentials entered during session creation are forwarded to Brain, then cleared from the form on success. The SDK code example uses environment-variable placeholders for credentials.

## Browse activity

The session list loads 50 entries at a time. Filters, ordering, and counts apply to the loaded entries. Use **Load more** or open a known session ID directly.

A session opens with its latest 200 retained events, then follows the event stream from that cursor. The conversation retains up to 500 events in memory; **Events** pages through the full retained history. Reconnecting resumes observation from the last cursor. An unconfirmed message keeps its idempotency key, so **Retry same message** checks the same submission. Select an attachment in a message to download it. **Export loaded events** downloads only the currently loaded activity.

**Usage** distinguishes the loaded activity from an explicitly requested full-history snapshot. Cumulative observations replace earlier snapshots, physical attempts are counted instead of their logical aggregate, and absent quantities remain unknown. The display reports tokens supplied by the provider, not monetary charges.

## Verify

Install the shared browser test dependency with `npm --prefix tools/browser ci --ignore-scripts` and `npm --prefix tools/browser run install-browser`. On Linux, also run `npm --prefix tools/browser exec -- playwright install-deps chromium`.

```sh
npm --prefix sites/dashboard run build
npm --prefix sites/dashboard test
```

The tests use an isolated HTTP fixture and a production Next server. Chromium exercises authentication, tenant isolation, session creation, attachments, streamed tool activity, reconnects, uncertain message acceptance, lifecycle actions, usage, and mobile navigation. No model credentials are needed. Screenshots are written to the ignored `artifacts/` directory.

The repository's full `node scripts/check.mjs` command also runs the service, SDK, and integration suites. CI runs the dashboard checks on Linux and Windows before publication is allowed.
