# Session preparation and declared Tool execution

The SDK exposes `clientBrowser({ name })` over the existing host mechanism. Session creation
still registers the host, opens its command stream and attaches local handlers automatically.
No wire, journal or execution-lifetime change accompanies the descriptive factory.

`sessions.prepare(options, hostId?)` admits supplied Components and returns the same create
request used by `sessions.create()`, without opening a host connection or creating a session.
Callers preparing host placements supply the host identity they will authorize. This lets a
hosting product authorize an exact composition before delegating creation to another client.
The returned request contains caller-supplied credentials and belongs in trusted server code.

`runToolHandler(inspectTool(placedTool), frame, signal?)` in `@aexhq/brain/runtime` runs an
already-declared native Tool through the same registry as the package runner. Adapters provide
the existing invocation services and return their actual durable acknowledgments. The helper
preserves output validation, result content, no-result completion and bounded cancellation.

SDK tests cover preparation without host I/O, invalid host identities, completion acknowledgment
ordering and cancellation. Existing session and package journeys continue to use the shared paths.
