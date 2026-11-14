# Agent runtimes

These packages integrate agent loops with Brain. Pi's Go adapter coordinates its
Sandbox environment, protocol, tool forwarding and settled checkpoints. Pi's
Node runner executes inside that environment. The embedded application API also
has Codex and OpenCode integrations.

`host/` owns the agent process protocol and shared process lifecycle helpers.
`host/protocol.json` is generated from its Go types for the Pi TypeScript runner.
`common/` holds runner utilities used across integrations.

The separately runnable `services/sandbox` service owns generic isolated
environments, process streams, resource limits, file transfer and cleanup. It
accepts process commands and does not interpret prompts, agent events or
checkpoints. Agent runtimes and hosted tools use the same generic Sandbox API.
