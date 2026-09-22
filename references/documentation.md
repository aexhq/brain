# Public documentation

Write for someone who has never seen Brain or Aex. Help them decide whether it solves
their problem, then get their first result. Accuracy comes from the code; structure comes
from the reader's task.

## Wording

- Brain runs AI agents and saves their conversations, tool results and progress. You run it.
- Aex hosts Brain for you. Bring your model key and connect your application.
- A session is one conversation and its work. A tool is a function an agent can call.
- An agent loop decides when to call the model or tools. An environment is where code runs.
- Explain benefits with observable behavior: read history, reconnect, add a tool, choose a model.
  Avoid unsupported promises about recovery, scale, languages or available extensions.

## Page design

| Surface | Reader's question | Order |
| --- | --- | --- |
| README and landing page | What is it? Why use it? | Purpose, benefits, first step, next tasks |
| Quickstart | How do I get a result? | Requirements, install, complete example, run, expected result |
| Concept | What does this mean for my app? | Plain definition, small usage example, relevant limits |
| Extension guide | How do I write one? | Small working example, supported languages, build, use, reference |
| Reference | What is the exact behavior? | Contracts and operational details, linked from the relevant task |

Keep one canonical guide per task. Brain owns its docs; Site imports them by revision.
Aex owns hosted setup and account docs. Link between them instead of repeating contracts.
Use language headings with source and build commands; distinguish client languages from
extension runtimes and tested examples from theoretical compatibility.

Keep pages short. Use ordinary words, runnable examples and descriptive links. Put prerequisites
before commands and explain where code runs when it affects deployment. Keep Wasmtime, compilation
internals, journals and contributor instructions out of introductions. Mention packaging only at
the build step. Retain limits that change a user's decision; link to the rest.

Review the README, package README, translated README, website and navigation together. Check that
examples use released versions, finish their work, show the result and link to a clear next step.
