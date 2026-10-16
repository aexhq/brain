# Brain documentation site

`website/` builds the static Brain documentation site using [Fumadocs](https://www.fumadocs.dev/), Next.js (static export), and MDX.

## Local development

Requires Go `1.26+`, Node.js `24+`, and `npm`. From the repository root:

```sh
npm --prefix runners/pi ci --ignore-scripts
npm --prefix website ci --ignore-scripts
npm --prefix website run dev
```

Open `http://127.0.0.1:3000/docs/`. `npm run dev` runs `website/scripts/generate.mjs` first to generate the OpenAPI endpoint pages, Go interface definitions, model catalog, and shared Markdown guides before starting Next.js. Re-run `npm --prefix website run generate` whenever you edit those upstream sources.

To build and verify the static export locally (requires Python 3 for the local preview server):

```sh
npm --prefix website run build
npm --prefix website test
npm --prefix website start
```

## Where content comes from

| Source | Generated / published output |
| --- | --- |
| `website/content/docs/**/*.mdx` | Authored guides and reference pages |
| `api/openapi.json` | `/docs/api/generated/*` and `/openapi.json` |
| `internal/harness` & `internal/persistence` | `/docs/adapters/interfaces` |
| Pinned Pi runtime model registry (`runners/pi`) | `/docs/reference/model-catalog` |
| `docs/running-persistence.md` | `/docs/guides/persistence` |
| `examples/tools/README.md` | `/docs/tools/local-tools` |
| `services/sandbox/README.md` | `/docs/tools/sandboxes` |
| `services/resources/README.md` | `/docs/tools/resources` |
| `examples/platforms/README.md` | `/docs/guides/platforms` |

`website/scripts/generate.mjs` writes all generated MDX files (which are gitignored—never edit them by hand). If you change Go route definitions or wire types in `internal/api` or `internal/core`, run `go run ./cmd/generate` from the repository root first.

## Static export

`npm --prefix website run build` writes the static site to `website/out/`, including:

- HTML pages under `/docs/*`
- Prebuilt search index at `/api/search`
- LLM-friendly Markdown exports at `/llms.txt`, `/llms-full.txt`, and `/llms.mdx/docs/*`

Serve `website/out/` with any static file host that resolves directory `index.html` files and preserves extensionless files (`/api/search`, `/llms.txt`, `/llms-full.txt`).

For a subpath deployment, set `NEXT_PUBLIC_BASE_PATH=/brain` before **both** `build` and `test`. CI tests both the root export and the GitHub Pages `/brain` subpath, then deploys the verified artifact after all checks pass. The public site is published at [aexhq.github.io/brain/docs](https://aexhq.github.io/brain/docs/). Keep `NEXT_PUBLIC_BASE_PATH` unset during local development.
