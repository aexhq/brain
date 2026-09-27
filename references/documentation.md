# Public documentation

Assume general familiarity with AI, LLMs and agents, but no prior knowledge of Brain or Aex.
Help readers decide whether it solves their problem, then get their first result. Accuracy comes
from the code; structure and abstraction come from the reader's task.

## Wording

- Brain runs agents with replaceable loops, models, tools and execution environments. You run it.
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
Use TypeScript first for application examples. Use language tabs for supported alternatives to
the same task, with shared prose outside the tabs and each language's imports/build commands
inside. One supported language needs no tabs. Extension languages do not imply application SDKs.

Keep pages short. Explain purpose, choices and non-obvious behavior; do not narrate obvious code.
Show the code the reader writes, expected results and relevant failures. State prerequisites
before use, define unfamiliar terms briefly and link background knowledge. Explain where code runs
when it affects deployment. Keep kernel structure, raw bindings and generators in contributor material.

Edit or merge existing pages before adding one. Keep one home for each setup, concept and contract.
Link to the complete runnable example; task excerpts may show only the relevant code if their prior
setup is clear. Delete stale explanations instead of adding corrections alongside them. Concision
must preserve necessary credentials, lifecycle limits and failure handling.

Review the README, package README, translated README, website and navigation together. Check that
examples use released versions, finish their work, show the result and link to a clear next step.
