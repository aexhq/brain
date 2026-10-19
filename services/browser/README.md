# Remote browser tool service

Run an isolated Chromium context for each trusted tenant/session pair. Brain calls this external service over authenticated HTTP. Browser state belongs to the service and expires after inactivity; your platform owns any longer retention.

```sh
npm --prefix services/browser ci --ignore-scripts
npm --prefix services/browser run install-browser
# Set TOOL_API_KEY to a random secret of at least 24 characters.
# Set BROWSER_ORIGINS to an exact JSON array, for example ["https://example.com"].
npm --prefix services/browser start
```

Install Chromium's OS dependencies on Linux with `npm --prefix services/browser exec -- playwright install-deps chromium`. Configure a Brain HTTP tool using the exported `descriptor` from `src/browser.mjs`, the endpoint `http://browser-tools:3005/browser`, the same service token, and your permitted tenants. The input is one `browser` tool with these actions:

| Action | Parameters | Result |
| --- | --- | --- |
| `navigate` | `url` | URL, title and visible text |
| `read` | Optional `selector` | URL, title and visible text |
| `click` | `selector` | Updated visible text |
| `fill` | `selector`, `value` | Updated visible text |
| `screenshot` | None | PNG image |
| `close` | None | Releases this session's context |

Only the authenticated envelope's tenant and session select the context. Model arguments cannot select another context or execute browser code. Each context has separate cookies and local storage. Downloads, service workers, popups and WebSockets are disabled. An owned network proxy enforces the exact HTTP/HTTPS origin allowlist on initial requests, subresources, redirects and HTTPS tunnels. Choose trusted origins and network isolation appropriate to your deployment; private origins are allowed only when explicitly configured.

The service admits eight contexts and eight HTTP calls by default, one active browser action per context. Idle contexts close after five minutes; selectors time out after ten seconds and the shared HTTP service cancels calls after ninety seconds. Text output is limited to 32,768 characters and PNG output to 1 MiB. Cancellation closes the affected context; a dispatched action that fails can return `unknown_outcome: true`. Inspect state before retrying. Closing a context or restarting the service creates no automatic continuation.

`browserService({ token, origins, capacity, idleMs })` embeds the same service for an application. `await service.close()` closes contexts, Chromium and the network proxy. Run `npm --prefix services/browser test` for real-browser network, isolation, capacity, idle expiry and cancellation checks. The installed-client service journey is in `tests/journeys/browser.test.mjs`.
