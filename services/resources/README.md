# Resource tools

`cmd/resource-tools` (`services/resources`) is a standalone Go HTTP tool service that provides public web fetching (`web_fetch`), web search (`web_search`), and tenant-scoped S3/R2 text storage (`storage_list`, `storage_read`, `storage_write`, `storage_delete`). It shares Brain's wire types and AWS SDK dependency while running outside the session server.

## Before you begin

- Go `1.26+`.
- `TOOL_API_KEY` set to a random secret of at least 24 characters (shared with Brain as `RESOURCE_TOOL_TOKEN`).
- Optional search credentials (`SEARCH_PROVIDER` and `SEARCH_API_KEY`) or S3/R2 storage settings (`STORAGE_BUCKET`, `STORAGE_REGION`).

## 1. Start the service and print descriptors

Start `cmd/resource-tools` (listening on `127.0.0.1:8092` by default) and generate the tool descriptors for your configured capabilities:

```sh
# Set TOOL_API_KEY to a random secret of at least 24 characters.
go run ./cmd/resource-tools
go run ./cmd/resource-tools -catalog http://127.0.0.1:8092 -tenant local
```

## 2. Register the tools in Brain

Copy the printed JSON descriptors into the `tools` array of your Brain configuration file (`brain.json`), and set `RESOURCE_TOOL_TOKEN` in Brain's environment to match the service's `TOOL_API_KEY`. The service binds to loopback (`127.0.0.1:8092`) by default; pass `-listen 0.0.0.0:8092` only behind TLS or private networking.

## Tools and configuration

| Tool | Configuration | Behavior |
| --- | --- | --- |
| `web_fetch` | No provider key | Public HTTP(S) on ports 80/443. DNS addresses are checked and pinned before dialing (rejecting private/loopback/link-local ranges), and redirects are validated at each hop. HTML is parsed into plain text. Upstream responses are capped at **2 MiB**; returned text is truncated at **128 KiB** with an explicit truncation flag. |
| `web_search` | `SEARCH_PROVIDER=brave` or `tavily`, `SEARCH_API_KEY` | Returns up to 10 results with titles, snippets, and source URLs. Uses standard HTTP without third-party search SDK dependencies. |
| `storage_list`, `storage_read` | `STORAGE_BUCKET`, `STORAGE_REGION`, optional `STORAGE_PREFIX` | Each tenant gets its own key prefix (`STORAGE_PREFIX/<base64url tenant_id>/`), assigned from the authenticated Brain scope. Lists paginate at 100 keys; reads accept UTF-8 text up to **128 KiB**. |
| `storage_write`, `storage_delete` | Also `STORAGE_ALLOW_WRITE=true` | Write and delete tools are disabled and omitted from `-catalog` output unless `STORAGE_ALLOW_WRITE=true` is explicitly set. |

### S3 and Cloudflare R2 storage settings

- **AWS S3:** Uses the default AWS credential chain unless `STORAGE_ACCESS_KEY` and `STORAGE_SECRET_KEY` are set.
- **Cloudflare R2:** Set `STORAGE_ENDPOINT=https://ACCOUNT_ID.r2.cloudflarestorage.com`, `STORAGE_REGION=auto`, `STORAGE_ACCESS_KEY`, and `STORAGE_SECRET_KEY`.
- **Bucket isolation:** Keep this bucket separate from Brain's checkpoint storage bucket.

## Security, scoping, and limits

- **Tenant isolation:** Object keys are validated relative paths. Storage is shared across sessions belonging to the same tenant (`tenant_id`), whereas sandbox handles and session-inspection tools use narrower per-session scope. The tool service trusts `tenant_id` only from the authenticated Brain request envelope, never from model-supplied arguments—the model cannot choose a bucket, credential, or another tenant's prefix.
- **Concurrency and timeouts:** The service admits at most **32** concurrent operations, and each request expires after **25 seconds**.
- **Failure semantics:** The service never retries model actions. An ambiguous `storage_write` or `storage_delete` transport failure returns a tool error (`is_error: true`) with `unknown_outcome: true`; ordinary search, fetch, or read failures return tool errors without implying a remote mutation.

> **Note:** Local tests verify the real AWS SDK transport, HTTP tool registry, search-provider fixtures, and private-network boundary checks. Live search and R2 tool accounts have not been qualified.

Sources: [Brave API](https://api-dashboard.search.brave.com/app/documentation/web-search/get-started), [Tavily API](https://docs.tavily.com/documentation/api-reference/endpoint/search), [R2 S3 compatibility](https://developers.cloudflare.com/r2/api/s3/api/).

## Next steps

- [External tools protocol](https://aexhq.github.io/brain/docs/tools)
- [Sandboxes tool service](https://aexhq.github.io/brain/docs/tools/sandboxes)
- [Local tools & approvals](https://aexhq.github.io/brain/docs/tools/local-tools)
- [Host configuration reference](https://aexhq.github.io/brain/docs/reference/configuration)
