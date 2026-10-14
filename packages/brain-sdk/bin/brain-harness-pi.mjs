#!/usr/bin/env node
process.env.PI_OFFLINE = "1";
console.log = console.error;
const { serveHarness } = await import("../dist/harness.js");
const { piHarness } = await import("../dist/harness-pi.js");
await serveHarness(piHarness());
