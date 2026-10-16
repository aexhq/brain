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
| L1 | **Sessions**: agent conversations that outlive connections and restarts. |
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

Say what Brain does, not how good it is. Agent and workflow products describe themselves with the same few words, and model-written text leans on the same vocabulary, so these words make Brain sound like every other product and tell the reader nothing. A fact only Brain can state says more than a quality any product can claim. Replace the word with the behavior behind it:

| Instead of | Write |
| --- | --- |
| Durable sessions | Sessions outlive client connections and server restarts. |
| Long-running agents | Agents that keep working after the client disconnects |
| Switches seamlessly to live events | Switches to live events without gaps or duplicates |
| Secure, multi-tenant tools | Each tool call carries the session's tenant, so your service can authorize it. |
| Pick up where you left off | Reconnect and continue from the last event you read. |
| Bring your own tools | Keep tools in your own services. |

### Words to avoid

| Kind | Avoid | Why |
| --- | --- | --- |
| Category words | durable, stateful, long-running, resilient, fault-tolerant, reliable, recoverable, scalable, at scale, secure (as a claim), orchestration, agentic, real-time, production-ready, enterprise-grade, battle-tested | Temporal, Inngest, Restate, LangGraph, Cloudflare Agents, and Amazon Bedrock AgentCore describe themselves with these words, so they don't set Brain apart. |
| Hype | powerful, seamless, robust, effortless, blazing fast, cutting-edge, state-of-the-art, next-generation, best-in-class, world-class, first-class, game-changing, revolutionary, magic, and superlatives such as "best" and "fastest" | They claim a quality without showing it. |
| Marketing verbs | unlock, empower, supercharge, unleash, elevate, streamline, leverage, utilize, revolutionize, harness (as a verb) | A plain verb says what happens: use, run, connect, send, store. |
| Minimizers | simply, just, easy, easily, simple, quickly, obviously, of course | They tell a reader who is stuck that the task should have been easy. |
| Model vocabulary | delve, crucial, pivotal, intricate, meticulous, tapestry, testament, landscape, realm, underscore (as a verb), highlight (as a verb), showcase, boasts, vibrant, foster, garner, enhance, key (as an adjective), comprehensive, additionally, moreover, furthermore | Studies of text published since late 2022 find these words far more often than before, and readers take them as a sign of unedited model output. |
| Filler | please note, note that, it's worth noting, it's important to note, at this time, in order to, very, really, actually, basically, in summary, overall | They delay the point. |
| Idioms and stock phrases | out of the box, under the hood, pick up where you left off, bring your own, batteries included, plug and play, turnkey, best of both worlds, heavy lifting, the tools you already use | Idioms don't translate, and stock phrases appear in every product's copy. |
| Temporary states | currently, for now, not yet, not supported, fixed at creation, frozen | They describe planned work, which belongs on the roadmap. |

Avoid these words on every page, with three exceptions:

- The roadmap (L4) and design pages (L5) can describe temporary states.
- Reference (L3) and design (L5) pages can use a word in a precise technical sense that the sentence makes clear. An adapter contract can require durable writes, meaning the data is on stable storage before the call returns.
- Product names keep their own words, such as Cloudflare Durable Objects.

### Patterns to avoid

- **Negative parallelism:** "Brain isn't just a server; it's a platform." State what Brain is. Use a contrast only to prevent a likely misreading, such as where a tool call's identity comes from.
- **Participle tails:** "Brain records every event, ensuring nothing is lost." A trailing "ensuring", "enabling", "allowing", or "making it easy to" attaches a vague benefit. Write the effect as its own clause: "Brain records every event, so clients can replay the history."
- **Copula avoidance:** "serves as", "acts as", "functions as", "boasts", "features", and "offers" in place of "is" and "has". Write "The harness is the agent loop", not "The harness serves as the agent loop".
- **Padding to three:** Stacked adjectives ("fast, flexible, and reliable") and lists stretched to three items for rhythm. List what exists, whether that's one item or five.
- **Openers and closers:** "Whether you're building X or Y", "Imagine…", "From X to Y", rhetorical questions, and closing summaries such as "In short". Start with the fact, and stop when the information ends.
- **Synonym cycling:** Calling one thing a session, a conversation, a thread, and a chat in turn. Use the word from [Terms](#terms) every time.
- **Decoration:** Emoji, exclamation marks, and Title Case headings. Use sentence case for headings.

`website/scripts/site.test.mjs` rejects the temporary states and the clearest of these words on the README, Introduction, and Architecture pages. The vocabulary drifts as products and models change. When a word shows up across several competing products or in model output, add it here, and add it to the test if it has no legitimate use on those pages.

Sources: Google's [voice and tone guide](https://developers.google.com/style/tone), Microsoft's [Use simple words, concise sentences](https://learn.microsoft.com/en-us/style-guide/word-choice/use-simple-words-concise-sentences), GitLab's [`Simplicity` lint rule](https://gitlab.com/gitlab-org/gitlab/-/blob/master/doc/.vale/gitlab_base/Simplicity.yml), Wikipedia's [Signs of AI writing](https://en.wikipedia.org/wiki/Wikipedia:Signs_of_AI_writing), and Kobak et al.'s [study of 15 million biomedical abstracts](https://doi.org/10.1126/sciadv.adt3813) (Science Advances, 2025).

## Terms

Use these words, defined once and used the same way everywhere:

- **Session:** an agent conversation with its own harness, model, instructions, and tools.
- **Turn:** the work Brain runs for one user message, until the harness finishes.
- **Event:** a typed record in a session's stream.
- **Harness:** the agent loop that calls the model and manages context. Brain includes the Pi harness.
- **Tool:** an endpoint in your own service that Brain calls when the model asks. The process that serves it is a **tool service**.
- **Tenant:** the isolation boundary for tokens, sessions, and tools.
- **Host config:** the JSON file that configures a Brain server.
- **Checkpoint:** saved session state for recovering or moving a session.

## Where pages come from

Some site pages are generated from Markdown files that also render on GitHub, including this repository's [roadmap](roadmap.md). Keep those files plain GitHub Markdown, and use full `https://aexhq.github.io/brain/docs/...` URLs for docs links. The Development page lists [which file to edit](https://aexhq.github.io/brain/docs/development/#where-to-edit-what) for each page.
