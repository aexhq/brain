import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";

import { buildToolModule, compileTools, tool } from "../dist/index.js";
import { canonicalJson } from "../dist/tools.js";
import { z } from "zod";

test("the local builder is deterministic and does not evaluate customer code", async () => {
  const directory = await mkdtemp(join(tmpdir(), "brain-builder-"));
  const previousMarker = process.env.BRAIN_BUILDER_TEST_MARKER;
  try {
    const marker = join(directory, "evaluated.txt");
    process.env.BRAIN_BUILDER_TEST_MARKER = marker;
    const modulePath = join(directory, "tool.mjs");
    await writeFile(
      modulePath,
      `import { writeFileSync } from "node:fs";\nwriteFileSync(process.env.BRAIN_BUILDER_TEST_MARKER, "bad");\nexport default { kind: "brain.tool" };\n`,
    );
    const url = pathToFileURL(modulePath).href;
    const first = await buildToolModule(url);
    const second = await buildToolModule(url);
    assert.equal(first.checksum, second.checksum);
    assert.deepEqual(first.bytes, second.bytes);
    await assert.rejects(readFile(marker), { code: "ENOENT" });
  } finally {
    if (previousMarker === undefined) delete process.env.BRAIN_BUILDER_TEST_MARKER;
    else process.env.BRAIN_BUILDER_TEST_MARKER = previousMarker;
    await rm(directory, { recursive: true, force: true });
  }
});

test("the conformance fixture has one cross-platform bundle identity", async () => {
  const fixture = new URL("../fixtures/fixture-tool.mjs", import.meta.url).href;
  assert.equal(
    (await buildToolModule(fixture)).checksum,
    "39c287a1cb4e62dcc28bcdad992d4f2277b761e888295e910ceb11bd440ac040",
  );
});

test("a managed bundle exports the canonical executable runtime once", async () => {
  const directory = await mkdtemp(join(tmpdir(), "brain-runtime-"));
  try {
    const prepared = await buildToolModule(new URL("../fixtures/fixture-tool.mjs", import.meta.url).href);
    const bundle = join(directory, "bundle.mjs");
    await writeFile(bundle, prepared.bytes);
    const runtime = (await import(`${pathToFileURL(bundle).href}?digest=${prepared.checksum}`)).default;
    assert.equal(runtime.kind, "brain.tool-runtime");
    assert.equal(runtime.contractDigest, "0123456789abcdef".repeat(4));
    assert.equal(typeof runtime.execute, "function");
    assert.deepEqual(
      await runtime.execute({ value: "a+b" }, {
        signal: new AbortController().signal,
        operationId: "op_test",
        sessionId: "ses_test",
        deadlineMs: Date.now() + 1_000,
        workspace: "/workspace",
      }),
      { value: "a+b", escaped: "a\\+b" },
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("custom and official values compile through one ordered Tool path", async () => {
  const module = new URL("../fixtures/fixture-tool.mjs", import.meta.url).href;
  const output = z.object({ value: z.string() });
  const one = tool(z.object({ value: z.string() }), async function one({ value }) { return { value }; })
    .describe("Return one.")
    .returns(output)
    .server(module);
  const two = tool(z.object({ value: z.string() }), async function two({ value }) { return { value }; })
    .describe("Return two.")
    .returns(output)
    .client({ registration: "callback.two" });
  const compiled = await compileTools([one, two]);
  assert.deepEqual(compiled.items.map((item) => item.definition.name), ["one", "two"]);
  assert.equal(compiled.items[0].executor.kind, "aex_managed");
  assert.equal(compiled.items[1].executor.kind, "customer_app");
  assert.equal(compiled.clientRegistrations[0].registration, "callback.two");
  assert.equal(compiled.clientRegistrations[0].handler, two.handler);
  assert.match(compiled.items[0].definition.contract_digest, /^[0-9a-f]{64}$/u);
  await assert.rejects(compileTools([one, one]), /selected more than once/u);
});

test("declared network needs ride the wire and refuse aex infrastructure", async () => {
  const module = new URL("../fixtures/fixture-tool.mjs", import.meta.url).href;
  const fetcher = tool(z.object({ url: z.string() }), async function fetcher({ url }) { return { url }; })
    .describe("Fetch a thing.")
    .server(module, {
      network: { destinations: [{ host: "api.example.com", ports: [443], protocol: "tls" }] },
    });
  const compiled = await compileTools([fetcher]);
  assert.deepEqual(compiled.items[0].network, {
    destinations: [{ host: "api.example.com", ports: [443], protocol: "tls" }],
  });

  assert.throws(
    () =>
      tool(z.object({}), async function infra() { return {}; })
        .server(module, {
          network: { destinations: [{ host: "api.aex.dev", ports: [443], protocol: "tls" }] },
        }),
    /always denied/u,
  );
});

test("function-first builders infer names and are immutable", async () => {
  const original = tool(async function ping() { return { ok: true }; });
  const described = original.describe("Check liveness.").returns(z.object({ ok: z.boolean() }));
  assert.notEqual(original, described);
  assert.equal(Object.isFrozen(original), true);
  const first = described.client();
  const second = described.client();
  assert.equal(first.name, "ping");
  assert.equal(first.contract.contractDigest, second.contract.contractDigest);
  assert.equal(first.executor.registration, second.executor.registration);
  assert.throws(() => tool(async () => 1).client(), /must be named/u);
});

test("the builder rejects runtime module discovery before session creation", async () => {
  const directory = await mkdtemp(join(tmpdir(), "brain-builder-dynamic-"));
  try {
    const modulePath = join(directory, "dynamic.mjs");
    await writeFile(
      modulePath,
      `export default { kind: "brain.tool", async execute() { return import(process.env.RUNTIME_MODULE); } };\n`,
    );
    await assert.rejects(
      buildToolModule(pathToFileURL(modulePath).href),
      /dynamic import|unsupported dynamic behavior/u,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("contract canonical JSON uses locale-independent RFC 8785 key ordering", () => {
  const value = {
    "\ufb33": "Hebrew Letter Dalet With Dagesh",
    "1": "One",
    "\ud83d\ude00": "Emoji: Grinning Face",
    "\u20ac": "Euro Sign",
    "\r": "Carriage Return",
    "\u00f6": "Latin Small Letter O With Diaeresis",
    "\u0080": "Control",
  };
  assert.equal(
    canonicalJson(value),
    '{"\\r":"Carriage Return","1":"One","":"Control","ö":"Latin Small Letter O With Diaeresis","€":"Euro Sign","😀":"Emoji: Grinning Face","דּ":"Hebrew Letter Dalet With Dagesh"}',
  );
  assert.throws(() => canonicalJson({ broken: Number.NaN }), /non-finite/u);
  assert.throws(() => canonicalJson({ broken: "\ud800" }), /unpaired/u);
  assert.throws(() => canonicalJson({ broken: undefined }), /cannot contain undefined/u);
});
