# Brain SDK

Connect your TypeScript or JavaScript application to [Brain](https://aex.dev/brain), the
open-source agent runtime. Connect models and tools, and drive agents through one session API.
The operator registers installed agentloops when starting Brain. Each session selects one by name
and supplies its behavior settings. Pi, Codex, and OpenCode implementations live in extension packages.

## Get started

The [quickstart](https://aex.dev/brain/docs/quickstart) has the server command, matching package
versions and a complete program. It runs an agent that checks an order through a function in your
app and prints the result.

| I want to… | Read |
| --- | --- |
| Send messages, stream output or collect submitted work | [Sessions](https://aex.dev/brain/docs/concepts/sessions) |
| Add app functions or packaged tools | [Write a tool](https://aex.dev/brain/docs/guides/write-a-tool) |
| Choose or write an agent loop | [Agent loops](https://aex.dev/brain/docs/concepts/agent-loop) |
| Run tools in a browser or remote service | [Environments](https://aex.dev/brain/docs/concepts/environment) |
| Choose a model | [Models](https://aex.dev/brain/docs/concepts/model) |

## Good to know

- **Tools run in your process.** Keep it running while the agent needs them. For short-lived
  requests, use [HTTP tools](https://github.com/aexhq/extensions/tree/main/packages/env-http) or
  another independently running environment.
- **`send()` waits for the session to go idle.** It returns session state, not the answer. Check the
  turn outcome to tell success from failure. Client timeouts don't cancel accepted work.
- **Idle clients disconnect.** The shared tool connection suspends after five idle seconds and
  reconnects before the same client starts more work. Set `connectionIdleTimeoutMs: 0` to keep
  tools reachable by other callers or future autonomous events. You own your event subscriptions.
- **Know what each call stops.** `session.interrupt()` stops current work, `session.end()` finishes
  the conversation, `session.delete()` removes its history and `brain.close()` permanently releases
  the client's connections.
- **Prepare at startup.** `await brain.prepare(placed)` prepares Tool code once so
  sessions can reuse it. See [Environment preparation](https://aex.dev/brain/docs/reference/environment-runtime#prepare-before-creating-sessions).

Prompt-based typed answers live in Aex's SDK, so Brain's `send()` and `submit()` reject `output` options.
Provider-native model formats and Tool schemas still work. Existing callers can follow the
[typed-answer migration](https://aex.dev/docs#structured-output).
