#!/usr/bin/env node
import { serveHarness } from "../dist/harness.js";
import { codexHarness } from "../dist/harness-codex.js";
await serveHarness(codexHarness());
