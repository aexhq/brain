# Brain SDK

Use TypeScript or JavaScript to connect your application to [Brain](https://aex.dev/brain), an
open-source agent runtime with pluggable loops, models, tools and execution environments. Choose
the loop, connect your tools and interact with the agent through one API.

## Get started

Follow the [quickstart](https://aex.dev/brain/docs/quickstart) for the server command, compatible
packages and complete TypeScript program. It runs an agent using an order lookup function in your
application and prints the result.

| Task | Guide |
| --- | --- |
| Send messages, stream output or retrieve submitted work | [Sessions](https://aex.dev/brain/docs/concepts/sessions) |
| Add application functions or packaged tools | [Tools](https://aex.dev/brain/docs/guides/write-a-tool) |
| Choose or write an agent loop | [Agent loops](https://aex.dev/brain/docs/concepts/agent-loop) |
| Place tools in a browser or remote service | [Environments](https://aex.dev/brain/docs/concepts/environment) |
| Choose a model | [Models](https://aex.dev/brain/docs/concepts/model) |

Application tools run in the connected application's process. For short-lived requests, use
[HTTP tools](https://github.com/aexhq/extensions/tree/main/packages/env-http) or another independently
running environment.

`send()` waits for the session to become idle; inspect the turn outcome to distinguish success from
failure. Client timeouts do not cancel accepted work. See [Sessions](https://aex.dev/brain/docs/concepts/sessions)
for outcome handling, errors and reconnecting.

The shared tool connection suspends after five seconds of inactivity and reconnects before
the same live client starts more work. Set `connectionIdleTimeoutMs: 0` to keep tools
available to other callers or future autonomous events. Event subscriptions stay caller-owned.

`brain.close()` permanently releases client connections, `session.interrupt()` stops work, `session.end()` finishes
the conversation, and `session.delete()` removes its history.

Prompt-based typed answers belong to Aex's SDK. Brain sends reject `output` options; provider-native
model formats and Tool schemas remain available. Existing callers can follow the
[typed-answer migration](https://aex.dev/docs#structured-output).

Prepare reusable Agentloop and Tool code before creating sessions with `await brain.prepare(placed)`.
[Environment preparation](https://aex.dev/brain/docs/reference/environment-runtime#prepare-before-creating-sessions)
explains startup configuration, shared runtimes and reuse across sessions.
