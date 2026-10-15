# Integration evidence

Reviewed 2026-10-15. Local tests and live qualification are separate release evidence.

| Integration | Evidence | Remaining limits |
| --- | --- | --- |
| AWS S3 archive | Live SDK object contract and full archive contract passed in eu-west-2: immutable writes, mutable pointer, cancellation, bounds, missing object, append efficiency, interrupted publication, cross-tenant rejection, restore into an empty directory, older checkpoint restore. Temporary bucket removed after the run. | This qualifies the tested standard bucket/IAM setup, not every S3-compatible service or policy. |
| R2 archive | Real AWS SDK transport fixture passes with R2 configuration. | No reusable R2 credentials available for a live run. |
| GCS archive | Official SDK multipart, precondition, read and shared object contract tests pass. | No ADC credentials available for a live run. |
| MCP | Official SDK client/server fixture: discovery, streamed progress, errors, structured content, tenant isolation, bounds, cancellation and no action replay. | No third-party server account qualified. |
| Modal, E2B, Daytona | Pinned SDK type checks; HTTP tool journey; signed handle ownership and restart; bounded remote helper; cancellation and cleanup fixtures. | Provider account secrets required for `npm --prefix services/sandbox run live -- PROVIDER`. |
| Resource tools | AWS SDK + HTTP registry journey; auth, tenant prefixes, read-only policy, delete; Brave/Tavily request fixtures; public-network and HTML extraction tests. | Live search/R2 resource-tool accounts remain unqualified. |

Run storage qualification with a dedicated bucket/prefix, `BRAIN_TEST_STORAGE_CONFIG` pointing to an absolute settings JSON file, and `go test -tags cloud -run TestCloudStorage -count=1 -v ./internal/persistence`. The explicit cloud profile fails if configuration or credentials are missing. Operators own cleanup of its isolated prefix after the run.

The Browser checks found an existing Modal token ID, but its secret is not retrievable from that page. E2B required a new GitHub OAuth grant, which was not authorized during browser automation. No new browser credentials or account permissions were created.
