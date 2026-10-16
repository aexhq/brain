# Writing Brain docs

Brain's documentation speaks at six levels, from a one-line pitch to design records. Each page uses one level, so readers get the detail they came for and nothing they didn't. This guide defines the levels and the rules that keep them apart.

## Levels

| Level | Answers | Surfaces | Voice | Kept out |
| --- | --- | --- | --- | --- |
| **L0 Pitch** | What is Brain, and why care? | GitHub description, README subtitle, site metadata, Introduction description | One line, outcome first | Protocols, config, numbers, internal nouns, caveats |
| **L1 Overview** | How should I think about it? What can I build? | README, Introduction, Architecture | Present tense, positive, every item at the same altitude | Field names, status codes, file paths, storage engines, tenancy mechanics, limits, planned work |
| **L2 Guide** | How do I do X? | Quickstart, guides, tool pages, SDK pages | Imperative, second person, task first | Exhaustive field tables, design rationale, "can't yet" disclaimers |
| **L3 Reference** | What exactly does X accept and return? | HTTP API, host configuration, events, errors, models | Exact, neutral, complete | Narrative, promises, "currently", "yet", "for now" |
| **L4 Roadmap** | What's next, and how mature is each part? | [Roadmap](roadmap.md) | Honest, forward-looking | Task breakdowns (link to the plans) |
| **L5 Design** | Why is it built this way? How do I change it? | ADRs, plans, research, Development, Adapters, this guide | Technical, rationale first | Nothing |

## Rules

1. **One level per page.** The README stays at L0 and L1. A guide links to the reference instead of restating it.
2. **Link down; never repeat down.** Higher levels point to the page that owns the detail.
3. **Describe what exists.** Positive statements set scope on their own: "Brain connects to remote MCP servers over Streamable HTTP" needs no list of what it doesn't connect to.
4. **Planned work lives on the roadmap.** When a gap is on the roadmap, other pages say nothing about it. The roadmap names it and says what to do today.
5. **State design choices as features.** Write "Every tool is a service you control", not "Brain has no built-in tools".
6. **Keep guarantees explicit.** Isolation and security properties are promises, not limitations: "A session-scoped token sees only its session."
7. **Don't overclaim.** L0 and L1 never imply that planned work exists.
8. **Reword facts for each level.** The same fact appears at several levels with different wording. Never paste a sentence from one level into another.

## One fact at each level

Session configuration, as the levels describe it:

| Level | Wording |
| --- | --- |
| L0 | (not mentioned) |
| L1 | **Sessions**: durable agent conversations that outlive connections and restarts. |
| L2 | Create a session with the harness, model, instructions, and tools the agent should use. |
| L3 | `config.tools` · `string[]` · Required · Names of the tools to enable, up to 128. |
| L4 | **Dynamic session configuration**: change a session's model, instructions, and tools between turns. Until then, create a new session. |
| L5 | Plan 05 proposes a `session.configure` event that applies only while the session is idle. |

## Patterns

- **L0:** Use one tagline everywhere: *A minimal and extensible agent server for building agent applications*. Change it in every L0 surface at once.
- **L1:** Open the README and the Introduction with the same pitch, which places Brain next to products readers know: *Brain is a single application you deploy as a service to run the agents behind your products. It's an open-source alternative to Claude Managed Agents and the OpenAI Agents SDK.* Then introduce each primitive as a bold, linked name plus one outcome clause. Present design choices as benefits. Show at most one short code example, and link every item to the page that owns its detail.
- **L2:** Use task-shaped headings ("Create a session"), numbered steps, and complete, runnable code. State the constraints the reader hits during the task as instructions. End with next steps.
- **L3:** Use tables (`Field | Type | Required | Default | Description`). Give exact limits, defaults, status codes, and validation rules.
- **L4:** For each item, say what you'll be able to do and what to do today. Label status as Available, Partly available, or Planned. Record how each integration was verified in the integration status table, and link to it from the integration's setup page.
- **L5:** Record the decision, its context, and its consequences.

## Words

On L0–L2 pages, avoid "currently", "for now", "not yet", "not supported", "fixed at creation", and "frozen". These words describe a temporary state, which belongs on the roadmap. `website/scripts/site.test.mjs` rejects them on the README, Introduction, and Architecture pages.

Everywhere, avoid "simply", "just", "easily", superlatives, and exclamation marks.

## Terms

Use these words, defined once and used the same way everywhere:

- **Session:** a durable agent conversation with its own harness, model, instructions, and tools.
- **Turn:** the work Brain runs for one user message, until the harness finishes.
- **Event:** a typed record in a session's stream.
- **Harness:** the agent loop that calls the model and manages context. Brain includes the Pi harness.
- **Tool:** an endpoint in your own service that Brain calls when the model asks. The process that serves it is a **tool service**.
- **Tenant:** the isolation boundary for tokens, sessions, and tools.
- **Host config:** the JSON file that configures a Brain server.
- **Checkpoint:** saved session state for recovering or moving a session.

## Where pages come from

Some site pages are generated from Markdown files that also render on GitHub, including this repository's [roadmap](roadmap.md). Keep those files plain GitHub Markdown, and use full `https://aexhq.github.io/brain/docs/...` URLs for docs links. The Development page lists [which file to edit](https://aexhq.github.io/brain/docs/development/#where-to-edit-what) for each page.
