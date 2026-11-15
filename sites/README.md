# Brain sites

| Site | Purpose | Local address |
| --- | --- | --- |
| [docs](docs/) | Documentation, API reference, and guides; static Next.js export | `http://127.0.0.1:3000/docs/` |
| [dashboard](dashboard/) | Session management and live activity; Next.js server | `http://127.0.0.1:3100/` |

Run `node scripts/setup.mjs` from the repository root to install both sites and their workspace dependencies. `node scripts/check.mjs` builds and tests both sites along with the service and SDK suites.

The docs deploy through the existing GitHub Pages workflow after CI passes. Run the dashboard as a Node.js service with access to your Brain deployment; its server forwards authenticated browser requests to Brain.
