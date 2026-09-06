# ADR-005: Keep WIT Components and use Wasmtime for the native runtime

- Decision date: 2026-08-26
- Status: Accepted
- Compiled: 2026-09-05

Supersedes: [ADR-004: Route all four extension kinds through Component worlds](2026-08-25-01-four-worlds.md).

## Context

The user requested a fresh evaluation of Wasmtime, Wasmer, and other language-neutral execution options. A pure synchronous Agentloop needed little more than typed input/output, while a general Component host must also handle imports, resources, and eventually asynchronous I/O.

## Decision

Keep WIT as the typed guest contract and Wasmtime as the native Component runtime. Do not build a Brain-owned general Component compatibility layer or require a separate JSON plugin framework. Source-language compilation stays outside the server contract.

## Alternatives considered

The August 26 Codex session reports that Wasmer 7.3 rejected native Component loading and the experimental bridge trapped at the first host import. A later Linux spike successfully ran WIT-generated canonical-ABI core Wasm directly: that route was viable for a narrow pure loop, so “Wasmer cannot run the algorithm” was not the conclusion. It required Brain to own lowering/lifting and hosting conventions. Extism added another plugin/JSON layer. The reported warm-call difference between Wasmer core and Wasmtime Components was too small to decide a network-bound workload.

## Consequences

This is a historical selection based on the versions and experiments discussed on August 26, not a current survey of competing runtimes. The raw benchmark archives are outside this task’s repository scope; no numerical performance claim is promoted here. [ADR-023: Give the Agentloop one whole turn and asynchronous Brain services](2026-09-04-01-turn-services.md) later requires async host services and [ADR-034: Run native Components in a separate capability-restricted Wasmtime worker](2026-09-05-03-wasm-worker.md) defines the supported native profile. Reconsider another engine if it preserves that contract with a demonstrated benefit.

## Sources

- Brain implementation/history: [6081ebb](https://github.com/aexhq/brain/commit/6081ebbf94f4dd38a7dfbd97c641c18f58efb187), [6d20d59](https://github.com/aexhq/brain/commit/6d20d59421f1ba137eeea6c471f44afffc758ceb), [a21ae6e](https://github.com/aexhq/brain/commit/a21ae6e5c46ea2bffd9fe96aebb45471f6f1ac45).
- Current reference: [crates/brain-loophost/wit/agentloop/agentloop.wit](../../crates/brain-loophost/wit/agentloop/agentloop.wit).
- Current reference: [crates/brain-loophost/Cargo.toml](../../crates/brain-loophost/Cargo.toml).
- [Codex session `01a03dd3-4217-7cc3-973c-8336841a6a26`](SOURCES.md#session-01a03dd3-4217-7cc3-973c-8336841a6a26), 2026-08-26T19:33:16.940Z: Assistant reports the initial Wasmer Component/bridge spike and its limits.
- [Codex session `01a03dd3-4217-7cc3-973c-8336841a6a26`](SOURCES.md#session-01a03dd3-4217-7cc3-973c-8336841a6a26), 2026-08-26T21:10:28.715Z: Assistant corrects the conclusion after the successful canonical-core Linux bakeoff.
- [Codex session `01a03dd3-4217-7cc3-973c-8336841a6a26`](SOURCES.md#session-01a03dd3-4217-7cc3-973c-8336841a6a26), 2026-08-26T22:07:55.979Z: User accepts the narrowed boundary and requests the final design and implementation specifications.

[Index](README.md) · [Source coverage and dating](SOURCES.md)
