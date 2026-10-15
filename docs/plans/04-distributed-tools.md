# 04: Distributed tools plan

- **Releases:** HTTP/SSE & approval example at R0; MCP at R2; first-party tool catalog at R3 ([Roadmap](../roadmap.md))

Connect harness tool calls to external services over HTTP, SSE, and (in R2) MCP. Brain validates schemas, attaches authenticated tenant/session identity, enforces timeouts and output limits, and records telemetry; external services own execution, approvals, sandboxes, and cleanup.

## Work items

| ID | Scope | Acceptance criteria |
| --- | --- | --- |
| **T1: HTTP/SSE transport** | Validate tool descriptors (`input_schema`, optional `output_schema`, `url`, `tenants`, `timeout_seconds`, `max_output_bytes`); send `tenant_id`, `session_id`, `call_id`, and `arguments`; parse JSON or SSE (`progress` + `result`). | Invalid inputs never reach the tool URL; responses are capped while reading (up to 2 MiB); redirects are rejected; dropped connections/timeouts set `unknown_outcome: true` without automatic retries. |
| **T2: Inspection & approvals** | Support `inspect_session: true` (short-lived read-only session token) and ship [`examples/tools`](../../examples/tools/README.md) (`echo`, `inspect_session`, `ask_approval`). | Delegated tokens can only read their own session and are revoked as soon as the tool call ends; approvals require no special endpoints or tables in Brain. |
| **T3: MCP adapter** | Integrate the [MCP Go SDK](https://github.com/modelcontextprotocol/go-sdk) so MCP servers share the same tenant scoping, error mapping, and timeout rules as HTTP tools. | MCP tools pass the same isolation, progress, cancellation, and error-boundary tests as HTTP/SSE tools. |
| **T4: Storage & web research** | Standalone tool services for S3/R2 buckets, web search ([Brave](https://api-dashboard.search.brave.com/app/documentation/web-search/get-started), [Tavily](https://docs.tavily.com/documentation/api-reference/endpoint/search)), and bounded URL fetching. | Bucket access stays isolated from Brain's internal checkpoint storage; URL fetcher enforces network/redirect policies and size/time bounds. |
| **T5: Sandboxes & compute** | External code-execution services backed by providers like [Fly Machines](https://docs.fly.io/machines/api/machines-resource) or [Modal Sandboxes](https://modal.com/docs/guide/sandboxes). | Services manage sandbox lifecycle, timeouts, and teardown outside Brain using standard HTTP/SSE or MCP tool calls. |
| **T6: Browser bridge** | External bridge service connecting authenticated browser tabs/sessions to agent tool calls. | Browser disconnects surface as ordinary `is_error` tool results without replaying mutations on reconnect. |
| **T7: Catalog packaging** | Package first-party tool services with generated descriptor configs and runnable examples. | Only tools listed in a session's `config.tools` are exposed to the model; every catalog package has an automated integration test. |
