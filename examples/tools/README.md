# Local external tools

This service runs separately from Brain and supplies `echo`, `inspect_session`, and `ask_approval`. It demonstrates the HTTP/SSE tool contract and tool-owned waiting. Approval state is ephemeral and bounded to 32 pending requests. A yes/no answer does not grant access to another service; an action service must enforce its own authorization.

Set distinct random tokens in the terminals running Brain and this service:

```powershell
$env:BRAIN_TOOLS_KEY = "a-distinct-random-tool-token-at-least-24-bytes"
$env:BRAIN_APPROVAL_KEY = "a-distinct-random-operator-token-at-least-24-bytes"
```

Generate a host configuration from the tool descriptors, then start the service:

```powershell
go run ./examples/tools -print-config | Set-Content -Encoding utf8 brain.json
go run ./examples/tools
```

In the Brain terminal, configure `BRAIN_API_KEY`, `OPENAI_API_KEY`, and the same `BRAIN_TOOLS_KEY`, then run `go run ./cmd/brain -config brain.json`. The generated configuration enables a filesystem checkpoint archive at `.brain-archive`. Both private data directories are ignored by Git.

Create a session with `config.tools` containing the selected names. `GET /catalog` lists the available descriptors. To use the existing session example:

```powershell
$env:BRAIN_TOOLS = "echo,inspect_session,ask_approval"
node examples/session.mjs "Ask me whether to continue, then report my answer."
```

An operator can list and answer pending questions from another terminal:

```powershell
$headers = @{ Authorization = "Bearer $env:BRAIN_APPROVAL_KEY" }
Invoke-RestMethod http://127.0.0.1:8081/approvals -Headers $headers
Invoke-RestMethod http://127.0.0.1:8081/approvals/CALL_ID -Method Post -Headers $headers -ContentType application/json -Body '{"approved":true}'
```

Only the operator token can list or answer questions. The tool token can invoke configured tools. Inspection receives a short-lived Brain credential scoped to the calling session; it is revoked when the tool finishes. The service uses only its configured Brain address and does not follow redirects with that credential.

Questions expire after five minutes. Cancellation or a lost connection removes the pending question. Denial and expiry become ordinary tool errors visible to the harness. There is no approval database, automatic action retry, or model heartbeat in Brain.
