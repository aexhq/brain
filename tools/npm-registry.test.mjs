import assert from "node:assert/strict";
import { test } from "node:test";
import { assertReleaseProvenance, registryValue, waitFor } from "./npm-registry.mjs";

function releaseProof() {
  const source = "1".repeat(40);
  return {
    predicateType: "https://slsa.dev/provenance/v1",
    subject: [{ name: "pkg:npm/%40aexhq/brain@0.41.0", digest: { sha512: "ab".repeat(64) } }],
    predicate: { buildDefinition: {
      externalParameters: { workflow: {
        repository: "https://github.com/aexhq/brain",
        path: ".github/workflows/npm-publish.yml",
        ref: `refs/tags/release/sha-${source}`,
      } },
      resolvedDependencies: [{ digest: { gitCommit: source } }],
    } },
  };
}

const releaseItem = {
  name: "@aexhq/brain", version: "0.41.0",
  integrity: `sha512-${Buffer.from("ab".repeat(64), "hex").toString("base64")}`,
};

test("a server-only release reuses the identical SDK's original release provenance", () => {
  assertReleaseProvenance(releaseProof(), releaseItem);
});

test("provenance must identify the exact reviewed package archive", () => {
  for (const item of [
    { ...releaseItem, name: "@aexhq/other" },
    { ...releaseItem, version: "0.42.0" },
    { ...releaseItem, integrity: `sha512-${Buffer.alloc(64).toString("base64")}` },
  ]) assert.throws(() => assertReleaseProvenance(releaseProof(), item), /reviewed archive/);
});

test("original provenance requires the Brain release workflow and matching immutable tag", () => {
  for (const mutation of [
    { repository: "https://github.com/other/brain" },
    { path: ".github/workflows/ci.yml" },
    { ref: "refs/heads/main" },
    { ref: `refs/tags/release/sha-${"2".repeat(40)}` },
  ]) {
    const proof = releaseProof();
    Object.assign(proof.predicate.buildDefinition.externalParameters.workflow, mutation);
    assert.throws(() => assertReleaseProvenance(proof, releaseItem), /immutable Brain release/);
  }
});

test("only missing registry metadata is treated as pending", () => {
  assert.equal(registryValue(() => '"sha512-ok"', "pkg@1.0.0", "dist.integrity"), "sha512-ok");
  assert.equal(registryValue(() => "", "pkg@1.0.0", "dist.integrity"), undefined);
  const failure = (code) => Object.assign(new Error(code), { stdout: JSON.stringify({ error: { code } }) });
  assert.equal(registryValue(() => { throw failure("E404"); }, "pkg@1.0.0", "dist.integrity"), undefined);
  for (const code of ["E401", "E403", "E500", "ETIMEDOUT"]) {
    assert.throws(() => registryValue(() => { throw failure(code); }, "pkg@1.0.0", "dist.integrity"), { message: code });
  }
  assert.throws(() => registryValue(() => "invalid JSON", "pkg@1.0.0", "dist.integrity"), SyntaxError);
});

function clock(t) {
  let now = 0;
  t.mock.method(Date, "now", () => now);
  t.mock.method(globalThis, "setTimeout", (callback, milliseconds) => {
    now += milliseconds;
    queueMicrotask(callback);
  });
  return () => now;
}

test("verification tolerates propagation beyond one minute", async (t) => {
  const now = clock(t);
  await waitFor(() => now() >= 125_000 ? "visible" : undefined, "visible", "archive");
  assert.equal(now(), 125_000);
});

test("permanent absence stops after five minutes with a verification-only recovery", async (t) => {
  const now = clock(t);
  await assert.rejects(waitFor(() => undefined, "visible", "archive"), /Re-run the failed verification job/);
  assert.equal(now(), 300_000);
});

test("verification does not retry integrity or authorization failures", async (t) => {
  const now = clock(t);
  await assert.rejects(waitFor(() => { throw new Error("different integrity"); }, "visible", "archive"), /different integrity/);
  assert.equal(now(), 0);
});
