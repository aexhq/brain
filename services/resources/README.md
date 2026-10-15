# Resource tools

Run web search, public web fetching, and tenant-scoped S3/R2 text storage as a separate Go service. It shares Brain's wire types and existing AWS SDK dependency, but does not run inside the session server.

```sh
# Set TOOL_API_KEY to a random secret of at least 24 characters.
go run ./cmd/resource-tools
go run ./cmd/resource-tools -catalog http://127.0.0.1:8092 -tenant local
```

Copy the printed descriptors into Brain's `tools` configuration. Set `RESOURCE_TOOL_TOKEN` on Brain to the same secret as the tool service's `TOOL_API_KEY`. The service binds to loopback by default; use `-listen 0.0.0.0:8092` only behind TLS/private networking.

| Tool | Configuration | Behavior |
| --- | --- | --- |
| `web_fetch` | No provider key | Public HTTP(S), ports 80/443. DNS addresses are checked and pinned before dialing; redirects are checked too. HTML is parsed into text. Responses are limited to 2 MiB; returned text is truncated at 128 KiB with an explicit flag. |
| `web_search` | `SEARCH_PROVIDER=brave` or `tavily`, `SEARCH_API_KEY` | Up to ten results with source URLs. No third-party search SDK dependency. |
| `storage_list`, `storage_read` | `STORAGE_BUCKET`, `STORAGE_REGION`, optional `STORAGE_PREFIX` | Tenant prefixes are assigned from authenticated Brain scope. Lists paginate at 100 keys; reads accept UTF-8 text up to 128 KiB. |
| `storage_write`, `storage_delete` | Also `STORAGE_ALLOW_WRITE=true` | Mutations are disabled and omitted from the catalog unless explicitly enabled by the operator. |

For AWS, use the default credential chain or `STORAGE_ACCESS_KEY` and `STORAGE_SECRET_KEY`. For R2 also set `STORAGE_ENDPOINT=https://ACCOUNT_ID.r2.cloudflarestorage.com` and `STORAGE_REGION=auto`. Keep this bucket separate from Brain checkpoint storage. The model cannot choose a bucket, credential, or another tenant's prefix.

Object names are relative paths. Storage is shared across sessions belonging to one tenant; sandbox handles and session-inspection tools use the narrower session scope. The tool service trusts identity only after authenticating its Brain host, never from model arguments.

Requests are bounded, admit at most 32 concurrent operations, and expire after 25 seconds. The service does not retry model actions. An ambiguous write/delete becomes a tool error with `unknown_outcome: true`; ordinary search/fetch/read failures remain tool errors without implying a mutation.

Local tests use the real AWS SDK transport and HTTP tool registry, plus search-provider fixtures and network-boundary checks. Live search and R2 tool accounts have not been qualified. Sources: [Brave API](https://api-dashboard.search.brave.com/app/documentation/web-search/get-started), [Tavily API](https://docs.tavily.com/documentation/api-reference/endpoint/search), [R2 S3 compatibility](https://developers.cloudflare.com/r2/api/s3/api/).
