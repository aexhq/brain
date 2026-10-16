# Sandbox tools

`services/sandbox` is an independent Node.js 24 HTTP tool service that wraps Modal, E2B, and Daytona behind four standard Brain tools: `sandbox_create`, `sandbox_exec`, `sandbox_inspect`, and `sandbox_terminate`.

Provider identifiers are wrapped in HMAC-signed handles scoped to the calling tenant and session. Keep `SANDBOX_SIGNING_KEY` stable across service restarts; no external ownership database is needed.

## Before you begin

- Node.js `24+` and `npm`.
- `TOOL_API_KEY` set to a random secret of at least 24 characters (shared with Brain as `SANDBOX_TOOL_TOKEN`).
- `SANDBOX_SIGNING_KEY` set to a random secret of at least 32 characters.
- Credentials for at least one sandbox provider (`modal`, `e2b`, or `daytona`).

## 1. Install and start the service

Install dependencies, start the service (listening on `127.0.0.1:8091` by default), and print the tool descriptors for the providers you configured:

```sh
npm --prefix services/sandbox ci --ignore-scripts
# Set TOOL_API_KEY (24+ characters), SANDBOX_SIGNING_KEY (32+ characters),
# and credentials for at least one provider below.
npm --prefix services/sandbox start
npm --prefix services/sandbox run catalog -- http://127.0.0.1:8091 local modal,e2b,daytona
```

## 2. Register the tools in Brain

Copy the generated JSON descriptors into the `tools` array of your Brain configuration file (`brain.json`) and set `SANDBOX_TOOL_TOKEN` in Brain's environment to match the service's `TOOL_API_KEY`.

- **Advertise only configured providers:** Pass only the providers you have configured to `npm --prefix services/sandbox run catalog`.
- **Network binding:** The service binds to loopback (`127.0.0.1:8091`) by default; set `HOST=0.0.0.0` only behind TLS or private networking.
- **Container image:** To build the standalone container image from the repository root, run `docker build -f services/sandbox/Dockerfile -t brain-sandbox-tools .`.

## Providers and lifecycles

| Provider | Required secrets | Optional host settings | Lifecycle |
| --- | --- | --- | --- |
| Modal | `MODAL_TOKEN_ID`, `MODAL_TOKEN_SECRET` | `MODAL_APP` (`brain-tools`), `MODAL_IMAGE` (`python:3.13-slim`) | Five-minute maximum lifetime; one CPU, 512 MiB. Uses the official JS SDK. |
| E2B | `E2B_API_KEY` | `E2B_TEMPLATE` (`code-interpreter-v1`) | Five-minute sandbox timeout. Inspect does not reconnect or resume it. |
| Daytona | `DAYTONA_API_KEY` | `DAYTONA_API_URL`, `DAYTONA_TARGET` (`us`) | Five-minute inactivity stop, then automatic deletion. Preview access is private. |

- **Handle lifetime:** Signed handles authorize `sandbox_exec` for 5 minutes. `sandbox_inspect` and `sandbox_terminate` remain available afterward so callers can clean up expired sandboxes.
- **Explicit provider lifecycles:** Daytona's provider timeout is an inactivity limit rather than a wall-clock lifetime. No tool resumes stopped sandboxes or extends handle lifetimes.

## Execution limits and failure semantics

- **Command execution (`sandbox_exec`):** Commands run inside the remote sandbox through a Python 3 helper with a **30-second** deadline and **64 KiB** combined `stdout`/`stderr` cap. On timeout or excess output, the helper kills the process group. Results include the exit status plus explicit truncation and timeout flags.
- **No automatic retries:** Provider failures and lost responses become tool errors (`is_error: true`) with `unknown_outcome: true` when the remote outcome is ambiguous; the service never retries a model action.
- **Disconnect behavior:** Disconnecting a client does not guarantee remote cancellation: an in-flight command retains its 30-second deadline, and the provider's sandbox expiry (see the table above) is the final cleanup bound.
- **Admission and isolation:** The service admits at most **16** concurrent HTTP requests. Keep provider account quotas and host CPU/memory limits appropriate for your workload. This common interface exposes no GPUs, public port tunnels, persistent disks, arbitrary container images, or cross-provider snapshots, and injects no credentials into sandboxes.

## Verification and security notes

Run the offline test suite to verify SDK types, HTTP lifecycle, authentication, cross-session isolation, handle tampering, service restarts, expiry, ambiguous failures, cleanup, and execution limits:

```sh
npm --prefix services/sandbox test
```

> **Note:** `npm --prefix services/sandbox test` runs local contract tests, not live-provider qualification.

With live provider credentials configured, run the live qualification script for `modal`, `e2b`, or `daytona`:

```sh
npm --prefix services/sandbox run live -- modal
```

The `live` script creates a sandbox, executes a command, inspects it, and terminates it in a `finally` block. Missing credentials fail explicitly (no silent skips), and running against a live account can incur provider charges.

> **Note:** Daytona `0.224.1` carries an upstream `braces` advisory through its file-glob dependencies (`GHSA-vfj7-8cjw-p6xm`), with no published fix at review time. This service never calls the Daytona SDK's upload/glob APIs or passes model input as glob patterns. Track the upstream fix and do not expand to those APIs without addressing it.

Sources: [Modal SDK](https://modal.com/docs/sdk/js), [E2B sandbox API](https://docs.e2b.dev/sdk-reference/js-sdk/v2.9.0/sandbox), [Daytona SDK](https://www.daytona.io/docs/en/typescript-sdk/).

## Next steps

- [External tools protocol](https://aexhq.github.io/brain/docs/tools)
- [Local tools & approvals](https://aexhq.github.io/brain/docs/tools/local-tools)
- [Storage & web resource tools](https://aexhq.github.io/brain/docs/tools/resources)
- [Host configuration reference](https://aexhq.github.io/brain/docs/reference/configuration)
