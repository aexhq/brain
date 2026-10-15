# Sandbox tools

An independent Node 24 HTTP tool service for Modal, E2B, and Daytona. Brain sees four ordinary tools: `sandbox_create`, `sandbox_exec`, `sandbox_inspect`, and `sandbox_terminate`. Provider identifiers are wrapped in signed handles scoped to the calling tenant and session. Keep the signing key stable across service restarts; no ownership database is needed.

```sh
npm --prefix services/sandbox ci --ignore-scripts
# Set TOOL_API_KEY (24+ characters), SANDBOX_SIGNING_KEY (32+ characters),
# and credentials for at least one provider below.
npm --prefix services/sandbox start
npm --prefix services/sandbox run catalog -- http://127.0.0.1:8091 local modal,e2b,daytona
```

Copy the generated descriptors into Brain's `tools` configuration. Set `SANDBOX_TOOL_TOKEN` on Brain to the service's `TOOL_API_KEY`. Only advertise providers you configured. Bind the service to loopback by default, or set `HOST=0.0.0.0` behind TLS/private networking. Build its separate image from the repository root: `docker build -f services/sandbox/Dockerfile -t brain-sandbox-tools .`.

| Provider | Required secrets | Optional host settings | Lifecycle |
| --- | --- | --- | --- |
| Modal | `MODAL_TOKEN_ID`, `MODAL_TOKEN_SECRET` | `MODAL_APP` (`brain-tools`), `MODAL_IMAGE` (`python:3.13-slim`) | Five-minute maximum lifetime; one CPU, 512 MiB. Uses the official JS SDK. |
| E2B | `E2B_API_KEY` | `E2B_TEMPLATE` (`code-interpreter-v1`) | Five-minute sandbox timeout. Inspect does not reconnect or resume it. |
| Daytona | `DAYTONA_API_KEY` | `DAYTONA_API_URL`, `DAYTONA_TARGET` (`us`) | Five-minute inactivity stop, then automatic deletion. Preview access is private. |

Handles authorize execution for five minutes. Inspection and termination remain available afterward for cleanup. Daytona's provider timeout is an inactivity limit, not a wall-clock lifetime. No tool resumes stopped sandboxes intentionally or extends handles. Provider lifecycle differences stay explicit.

Commands run in the remote sandbox through a Python 3 helper, with a 30-second deadline and 64 KiB combined stdout/stderr. On timeout or excess output it kills the process group. Results include exit status and truncation/timeout flags. Provider failures and lost responses are tool errors with unknown outcomes; the service never retries a model action. Disconnecting a client does not promise remote cancellation: execution retains its deadline, and sandbox expiry is the final cleanup bound.

The service admits at most 16 active HTTP requests. Keep provider account quotas and deployment memory/CPU limits appropriate to your users. There are no GPUs, public port tunnels, persistent disks, arbitrary images, or cross-provider snapshots in this common interface. No credentials are injected into sandboxes.

## Verification

`npm --prefix services/sandbox test` checks the actual SDK types, the HTTP lifecycle, authentication, cross-session isolation, tampering, service restart, expiry, ambiguous failures, cleanup, and execution limits. These are local tests, not live-provider qualification.

With credentials, run `npm --prefix services/sandbox run live -- modal` (or `e2b`, `daytona`). It creates a sandbox, executes a command, inspects it, and terminates it in a `finally` block. Missing credentials fail; there are no silent skips. Account usage can incur provider charges.

Daytona 0.224.1 has an upstream `braces` advisory through its file-glob dependencies (GHSA-vfj7-8cjw-p6xm), with no published fix at review time. This service never calls the SDK's upload/glob APIs or passes model input as glob patterns. Track the upstream fix; do not expand to those APIs without addressing it.

Sources: [Modal SDK](https://modal.com/docs/sdk/js), [E2B sandbox API](https://docs.e2b.dev/sdk-reference/js-sdk/v2.9.0/sandbox), [Daytona SDK](https://www.daytona.io/docs/en/typescript-sdk/).
