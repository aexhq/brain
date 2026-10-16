# Local external tools

`examples/tools` is a standalone Go service (`127.0.0.1:8081` by default) that demonstrates Brain's HTTP JSON and SSE tool protocol, delegated session inspection, and operator approval gates.

| Tool | Response mode | Description |
| --- | --- | --- |
| `echo` | HTTP JSON (`application/json`) | Returns the caller's input text. |
| `inspect_session` | HTTP JSON (`application/json`) | Reads the calling session's status and recent event history using a short-lived, read-only `inspection_token` and `brain_url` delegated by Brain (`inspect_session: true`). |
| `ask_approval` | Streaming SSE (`text/event-stream`) | Emits `event: progress` frames while waiting up to 5 minutes for an operator to approve or deny the tool call over HTTP. |

## Before you begin

- Go `1.26+` and Node.js `24+` installed (`npm --prefix runners/pi run build` and `npm --prefix sdks/typescript run build` completed).
- An LLM provider API key (for example, `OPENAI_API_KEY`).
- Three distinct random tokens of at least 24 bytes each:
  - `BRAIN_API_KEY` — authenticates client requests to Brain.
  - `BRAIN_TOOLS_KEY` — authenticates tool dispatch requests from Brain to `examples/tools`.
  - `BRAIN_APPROVAL_KEY` — authenticates operator approval requests to `examples/tools`.

## 1. Start the tool service

Set `BRAIN_TOOLS_KEY` and `BRAIN_APPROVAL_KEY`, generate a ready-to-use `brain.json` with `-print-config` (which binds all three tools and configures a local `.brain-archive` checkpoint directory), and start the tool server:

```powershell
$env:BRAIN_TOOLS_KEY = "a-distinct-random-tool-token-at-least-24-bytes"
$env:BRAIN_APPROVAL_KEY = "a-distinct-random-operator-token-at-least-24-bytes"
go run ./examples/tools -print-config | Set-Content -Encoding utf8 brain.json
go run ./examples/tools
```

## 2. Start Brain

In a second terminal, set `BRAIN_API_KEY`, `OPENAI_API_KEY`, and the same `BRAIN_TOOLS_KEY`, then start Brain with the generated `brain.json`:

```powershell
$env:BRAIN_API_KEY = "replace-with-a-random-token-of-at-least-24-bytes"
$env:OPENAI_API_KEY = "your-provider-key"
$env:BRAIN_TOOLS_KEY = "a-distinct-random-tool-token-at-least-24-bytes"
go run ./cmd/brain -config brain.json
```

## 3. Run a session with tools

Pass the tool names in `config.tools` when creating a session (or call `GET /catalog` to inspect available tools). Using the runnable TypeScript example script:

```powershell
$env:BRAIN_API_KEY = "replace-with-a-random-token-of-at-least-24-bytes"
$env:BRAIN_TOOLS = "echo,inspect_session,ask_approval"
node examples/session.mjs "Ask me whether to continue, then report my answer."
```

## 4. Approve or deny a pending request

When the model calls `ask_approval`, the tool service holds the SSE stream open (up to 5 minutes, maximum 32 concurrent pending requests). From a third terminal, use `BRAIN_APPROVAL_KEY` to list pending requests and approve or deny the call by `CALL_ID`:

```powershell
$headers = @{ Authorization = "Bearer $env:BRAIN_APPROVAL_KEY" }
Invoke-RestMethod http://127.0.0.1:8081/approvals -Headers $headers
Invoke-RestMethod http://127.0.0.1:8081/approvals/CALL_ID -Method Post -Headers $headers -ContentType application/json -Body '{"approved":true}'
```

> **Note:** If an operator denies the request (`{"approved":false}`) or it expires after 5 minutes, the tool service returns an ordinary `is_error: true` tool result to the model so the turn can continue cleanly. Interrupting or closing the session drops the SSE connection and clears the pending request automatically.

## Next steps

- [External tools protocol](https://aexhq.github.io/brain/docs/tools)
- [Sandboxes tool service](https://aexhq.github.io/brain/docs/tools/sandboxes)
- [Storage & web resource tools](https://aexhq.github.io/brain/docs/tools/resources)
- [MCP servers](https://aexhq.github.io/brain/docs/tools/mcp)
