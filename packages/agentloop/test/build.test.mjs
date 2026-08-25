import assert from "node:assert/strict";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { buildLoopBundle, lintLoopBundle } from "../dist/build.js";

const fixture = (name) => fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url));

test("a loop entry bundles deterministically with the host binding injected", async () => {
  const first = await buildLoopBundle({ entry: fixture("probe-loop.mjs") });
  const second = await buildLoopBundle({ entry: fixture("probe-loop.mjs") });
  assert.equal(first.sha256, second.sha256, "the sealed identity must be reproducible");
  assert.equal(first.bytes, Buffer.byteLength(first.source, "utf8"));
  assert.match(first.source, /loophost:abi\/host/, "the host import stays external");
  assert.match(first.source, /activate/, "the activate export survives bundling");
  assert.ok(
    !first.source.includes("@aexhq/agentloop"),
    "the SDK is bundled in, not left as an import the guest cannot resolve",
  );
});

test("the gate refuses Unicode property escapes with an explicit override", async () => {
  await assert.rejects(
    buildLoopBundle({ entry: fixture("unicode-loop.mjs") }),
    /Unicode property escapes/,
  );
  const overridden = await buildLoopBundle({
    entry: fixture("unicode-loop.mjs"),
    allowUnicodePropertyEscapes: true,
  });
  assert.ok(overridden.sha256.length === 64);
});

test("the lint gate names offending lines", () => {
  assert.throws(
    () => lintLoopBundle("const ok = 1;\nconst re = /\\p{L}/u;\n"),
    /line\(s\) 2/,
  );
  lintLoopBundle("const fine = 'no escapes here';\n");
});
