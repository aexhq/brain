# Brain documentation site

The docs site is built with [Fumadocs](https://www.fumadocs.dev/), Next.js (static export), and MDX.

## Local development

Requires Go 1.26+, Node.js 24+, and npm. From the repository root:

```sh
npm --prefix runners/pi ci --ignore-scripts
npm --prefix website ci --ignore-scripts
npm --prefix website run dev
```

Open `http://127.0.0.1:3000/docs/`. `npm run dev` generates the API reference, Go interface docs, model catalog, and shared guides before starting Next.js. Rerun `npm --prefix website run generate` whenever you change those upstream sources.

To build and test the static export (requires Python 3 for the local preview server):

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
| Pinned Pi runtime model registry | `/docs/reference/model-catalog` |
| `docs/running-persistence.md` | `/docs/guides/persistence` |
| `examples/tools/README.md` | `/docs/tools/local-tools` |
| `services/*/README.md` | Sandbox and resource tool guides |
| `examples/platforms/README.md` | `/docs/guides/platforms` |

`website/scripts/generate.mjs` writes all generated MDX files (which are gitignored—do not edit them by hand). If you change Go route definitions or wire types, run `go run ./cmd/generate` from the repo root first.

## Static export

`npm run build` outputs static files to `website/out/`, including:
- HTML pages under `/docs/*`
- Prebuilt search index at `/api/search`
- LLM-friendly Markdown exports at `/llms.txt`, `/llms-full.txt`, and `/llms.mdx/docs/*`

Serve `website/out/` with any static file host that supports `index.html` resolution and preserves extensionless files (`/api/search`, `/llms.txt`, `/llms-full.txt`).

For a subpath deployment, set `NEXT_PUBLIC_BASE_PATH=/brain` before **both** build and tests. CI tests the root export and the GitHub Pages subpath, then deploys the exact tested Pages artifact after all verification jobs pass. The public URL is [aexhq.github.io/brain/docs](https://aexhq.github.io/brain/docs/). Keep local development's base path empty.
