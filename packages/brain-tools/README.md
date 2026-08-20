# `@aexhq/brain-tools`

Portable Tool values for Brain. Nothing is granted by default; import and select only the values a
session should expose.

```ts
import { bash, edit, read } from "@aexhq/brain-tools";

const session = await brain.sessions.create({
  model,
  tools: [read, edit, bash],
});
```

These values use the same public `Tool` type and create-session path as third-party packages.
