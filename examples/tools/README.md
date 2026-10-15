# Local external tools

`examples/tools` is a standalone Go service that demonstrates Brain's HTTP and SSE tool protocol. It provides three example tools:

- `echo` — returns the input text over HTTP JSON.
- `inspect_session` — reads the calling session's status and history using a short-lived, read-only token delegated by Brain.
- `ask_approval` — streams SSE progress while waiting for an operator to approve or deny the request over HTTP.

## 1. Start the tool service

Set two random tokens (at least 24 bytes each): `BRAIN_TOOLS_KEY` authenticates calls from Brain, and `BRAIN_APPROVAL_KEY` authenticates operator approval commands.

```powershell
$env:BRAIN_TOOLS_KEY = "a-distinct-random-tool-token-at-least-24-bytes"
$env:BRAIN_APPROVAL_KEY = "a-distinct-random-operator-token-at-least-24-bytes"
go run ./examples/tools -print-config | Set-Content -Encoding utf8 brain.json
go run ./examples/tools
```

`go run ./examples/tools -print-config` writes a ready-to-use `brain.json` with the three tool descriptors and a local `.brain-archive` checkpoint directory configured.

## 2. Start Brain

In a second terminal, set `BRAIN_API_KEY`, `OPENAI_API_KEY`, and the same `BRAIN_TOOLS_KEY`, then start Brain with the generated config:

```powershell
$env:BRAIN_API_KEY = "replace-with-a-random-token-of-at-least-24-bytes"
$env:OPENAI_API_KEY = "your-provider-key"
$env:BRAIN_TOOLS_KEY = "a-distinct-random-tool-token-at-least-24-bytes"
go run ./cmd/brain -config brain.json
```

## 3. Run a session with tools

Pass the tool names in `config.tools` when creating a session (or inspect `GET /catalog` to see available tools). Using the example script:

```powershell
$env:BRAIN_API_KEY = "replace-with-a-random-token-of-at-least-24-bytes"
$env:BRAIN_TOOLS = "echo,inspect_session,ask_approval"
node examples/session.mjs "Ask me whether to continue, then report my answer."
```

## 4. Approve or deny a pending request

When the model calls `ask_approval`, the tool service holds the SSE stream open (up to 5 minutes, max 32 pending requests). From another terminal, use `BRAIN_APPROVAL_KEY` to list and answer pending requests:

```powershell
$headers = @{ Authorization = "Bearer $env:BRAIN_APPROVAL_KEY" }
Invoke-RestMethod http://127.0.0.1:8081/approvals -Headers $headers
Invoke-RestMethod http://127.0.0.1:8081/approvals/CALL_ID -Method Post -Headers $headers -ContentType application/json -Body '{"approved":true}'
```

If an operator denies the request or it expires after 5 minutes, the tool returns an ordinary `is_error: true` tool result to the model. Cancelling the session or dropping the connection clears the pending request automatically.
