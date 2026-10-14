#!/usr/bin/env bun
console.log = console.error;
const { serveHarness } = await import("../dist/harness.js");
const { openCodeHarness } = await import("../dist/harness-opencode.js");
await serveHarness(openCodeHarness());
