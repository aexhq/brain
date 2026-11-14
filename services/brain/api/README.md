# Brain HTTP contract

This directory owns Brain's public Go session types and generated `openapi.json`.
OpenAPI describes HTTP operations for the TypeScript client and documentation site.
It is unrelated to an OpenAI model-provider configuration.

The sources are `types.go` and `../internal/http/contract.go`. The server registers
the same route declarations used by the generator. Huma derives JSON Schema from
the Go types; the generator supplies operation metadata for JSON, SSE, binary
attachments and executor WebSocket upgrades. Existing HTTP handlers own strict
decoding, authorization, error responses and streaming lifecycles.

From the repository root:

```sh
go run ./services/brain/cmd/generate
node sdks/typescript/scripts/generate.mjs
```

The Go command writes this contract and `runtimes/host/protocol.json`. The Node
command uses openapi-typescript to generate the SDK types and Pi protocol types.
Runtime protocol schemas have their own source in `runtimes/host`; they are not
HTTP endpoints. Generated aliases preserve readable public type names.

The SDK uses openapi-fetch for typed session requests, with owned error mapping,
telemetry and cancellation. SSE parsing and executor connections remain owned
code. Next.js/Fumadocs consumes this contract and publishes `/openapi.json`.

`node scripts/check.mjs` checks both generation stages for drift before building
and testing consumers. For just those checks, pass `-check` to the Go command and
`--check` to the Node command. Do not edit generated files.
