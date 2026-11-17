# Brain intents

This is the source of truth for design intent. Code and tests describe implemented behavior; the [roadmap](roadmap.md) tracks gaps. Historical ADRs and plans provide context, not additional requirements.

Direct requests, corrections and focused question-and-answer decisions carry more weight than broad approval of a long proposal. Newer explicit corrections take precedence. Derived choices must remain revisable rather than becoming product requirements through repetition.

## 1. Product intent

Brain is an open-source managed agent platform: a multiplexed, session-based Go brain server. Separate the brain from the hands, using existing sandbox and tool providers.

- Accept sessions, messages, attachments and references. Session IDs come from callers.
- Keep session data and transcripts across execution lifetimes. Read through memory, local disk and external storage behind one interface, adapting to capacity and TTL. Store metadata on disk and synchronize it externally.
- Support three tool flavours: uploaded execution (`HostedTool`), remote execution (`RemoteTool`) and user-client execution (`BrowserTool`), presented through one canonical tool interface.
- Use Pi as the agent runtime for this design, with an isolated environment for each session.
- Provide a TypeScript SDK, CLI and session dashboard. Use Docker for deployment.

## 2. Ownership and boundaries

Keep responsibilities cohesive and interfaces small. Layout should make ownership apparent.

| Component | Owns |
| --- | --- |
| Brain | Session lifecycle, coordination and requests to the agent runtime |
| Agent runtime | Inference, context and the agent loop |
| Tools | Tool bindings, authorization and dispatch across the three flavours |
| Storage | All storage tiers, adapters, persistence, caching, synchronization and storage resource lifecycle |
| Sandbox | Generic isolated environments, processes and execution resources |

Brain initiates storage operations through Storage primitives. Storage owns the entire memory tier as well as disk and external storage; working copies, locks and cleanup belong there. Backends share a unified adapter interface.

Storage also owns encryption and decryption of persisted credentials, transparently to callers. Brain and Tools use credentials for their respective work without handling ciphers or separate encryption keys. They operate within the same trusted application boundary. Credentials stay out of public session output, model arguments, transcripts and telemetry.

Component separation expresses code ownership. Storage is a package responsibility, not a requirement for an independent service. The earlier request for independently runnable services does not require separate processes or security boundaries for every component. Sandbox remains generic and separate from agent-specific behavior; it runs session runtimes and hosted tools in isolated environments.

## 3. Concrete preferences and derived choices

These preferences refine the intent above without fixing every implementation detail:

- Keep private contracts and helpers beside their owner. Multiple callers alone do not justify a root shared package. Put local test fixtures beside their tests.
- Put supplied integrations such as Modal/E2B under `tools/`; implement them through the same interfaces available to user-written tools.
- Group external SDKs and CLI under `clients/`, and documentation and dashboard under `sites/`.
- Treat telemetry as a shared contract available to services, SDK and CLI; shared service code belongs under `services/shared/`.
- Prefer standard-library facilities and established libraries over custom equivalents. Keep abstractions proportional to the problem.

Process topology, internal protocols, key formats and concrete storage algorithms are implementation choices. A proposed durability guarantee or accepted data-loss window needs a focused decision; broad implementation approval does not establish one. Preserve working behavior while changes are designed, and record remaining work on the roadmap rather than expanding this document into an implementation inventory.
