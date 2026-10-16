# Working on Brain

Follow `docs/adr/0001-local-first-session-server.md`. Code and tests own implemented behavior; plans describe work still to do.

- Keep the core small. Harnesses own inference and context; external services own tools and approvals.
- Fail explicitly. Tool errors become tool results; Brain errors stop the affected execution.
- Before adding a check or layer, identify a reachable failure and what changes when it fires.
- No versioning or backward compatibility unless explicitly requested. Pin dependencies for reproducibility.
- Run all required checks. Never skip tests or treat missing integration credentials as success.
- Commit complete, verified slices. Do not commit credentials or local session data.
- Write docs at the level `docs/writing.md` assigns to each page. Planned work and its current gaps belong on the roadmap.

