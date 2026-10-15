# Brain documentation

The documentation site uses [Fumadocs](https://www.fumadocs.dev/), Next.js static export, and MDX. Its monochrome layout takes inspiration from [Better Auth's documentation](https://better-auth.com/docs), with Brain's own branding and content. It runs separately from the Brain server and adds no server runtime dependencies.

## Local development

Use Go 1.26+, Node.js 24, and npm. From the repository root:

```sh
npm --prefix runners/pi ci --ignore-scripts
npm --prefix website ci --ignore-scripts
npm --prefix website run dev
```

Open http://127.0.0.1:3000/docs/. The dev command generates reference pages before starting Next.js; authored MDX updates live. Rerun `npm --prefix website run generate` after changing generated inputs.

To preview the production export (Python 3 required):

```sh
npm --prefix website run build
npm --prefix website test
npm --prefix website start
```

## Content and generation

| Source | Published output |
| --- | --- |
| `content/docs/*.mdx` and subdirectories | Guides, usage, configuration, events, errors, and adapter documentation |
| Root `api/openapi.json` | Endpoint pages and downloadable `/openapi.json` |
| Root Go harness and persistence declarations | Adapter interfaces |
| Pinned Pi runtime's offline model registry | Model catalog; availability does not imply live-provider qualification |
| Root `docs/running-persistence.md` | Persistence and recovery guide |
| Root `examples/tools/README.md` | Local tool and approval walkthrough |

`scripts/generate.mjs` owns generated pages, which are ignored by Git. Do not edit them directly. Regenerate the root API contract with `go run ./cmd/generate` when Go contracts change, then build the site. The model generator disables network refresh and local overrides and does not need provider credentials.

Navigation order lives in each content directory's `meta.json`. The content loader is `lib/source.ts`; the shared layout and GitHub source branch live in `lib/layout.shared.tsx` and `lib/shared.ts`. `app/global.css` owns Brain's visual styling. Change the source branch when the repository transition is complete.

## Static hosting

`npm run build` produces `out/`. Serve that directory with index-file support, preserving extensionless files such as `/api/search`, `/llms.txt`, and `/llms-full.txt`. No Node.js service, database, search service, or provider credentials are needed to serve the output. The OpenAPI reference shows request examples and schemas; execution stays in the developer's terminal or SDK because Brain does not expose browser CORS by default.

Every page has a Markdown export under `/llms.mdx/docs/`. API Markdown includes the operation and its referenced schemas. Search uses a bundled static index. Nothing is published automatically by this repository.

## Verification

The root `node scripts/check.mjs` runs the documentation build and tests alongside the server, runner, SDK, and journey checks. Site tests validate internal links and anchors, API contract parity, schema references, search coverage, and Markdown exports. Builds also type-check the application. CI runs on Linux and Windows.
