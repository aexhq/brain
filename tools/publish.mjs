import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";

const npmCli = [
  process.env.npm_execpath,
  path.join(path.dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js"),
  path.resolve(path.dirname(process.execPath), "../lib/node_modules/npm/bin/npm-cli.js"),
].find((candidate) => candidate !== undefined && existsSync(candidate));
if (npmCli === undefined) throw new Error("could not locate npm-cli.js for the active Node runtime");

const directory = import.meta.dirname;
const manifest = JSON.parse(readFileSync(path.join(directory, "manifest.json"), "utf8"));
const expectedCommit = process.env.EXPECTED_COMMIT ?? "";
if (!/^[0-9a-f]{40}$/u.test(expectedCommit) || manifest.source !== expectedCommit) {
  throw new Error("the release archive source does not match EXPECTED_COMMIT");
}

const run = (args, stdio = "pipe", cwd = directory) => {
  const output = execFileSync(process.execPath, [npmCli, ...args], {
    cwd,
    encoding: "utf8",
    stdio,
  });
  return typeof output === "string" ? output.trim() : "";
};

const registryValue = (spec, field) => {
  try {
    return JSON.parse(run(["view", spec, field, "--json"]));
  } catch {
    return undefined;
  }
};

const waitFor = async (read, expected, description) => {
  for (let attempt = 0; attempt < 12; attempt += 1) {
    if (read() === expected) return;
    await new Promise((resolve) => setTimeout(resolve, 5_000));
  }
  throw new Error(`${description} did not become ${JSON.stringify(expected)} within 60 seconds`);
};

const assertRegistryObject = (item) => {
  const spec = `${item.name}@${item.version}`;
  const integrity = registryValue(spec, "dist.integrity");
  if (integrity !== item.integrity) {
    throw new Error(
      integrity === undefined
        ? `${spec} is not visible on the public registry`
        : `${spec} exists with integrity ${integrity}, not this release's ${item.integrity}`,
    );
  }
};

const assertOriginalProvenance = async (item) => {
  const spec = `${item.name}@${item.version}`;
  const metadata = registryValue(spec, "dist.attestations");
  if (
    metadata?.provenance?.predicateType !== "https://slsa.dev/provenance/v1" ||
    typeof metadata.url !== "string" ||
    !metadata.url.startsWith("https://registry.npmjs.org/")
  ) {
    throw new Error(`${spec} has no npm SLSA provenance attestation`);
  }
  const response = await fetch(metadata.url, { redirect: "error" });
  if (!response.ok) throw new Error(`${spec} provenance endpoint returned ${response.status}`);
  const document = await response.json();
  const attestation = document.attestations?.find(
    ({ predicateType }) => predicateType === "https://slsa.dev/provenance/v1",
  );
  if (attestation === undefined) throw new Error(`${spec} has no SLSA v1 bundle`);
  const payload = JSON.parse(
    Buffer.from(attestation.bundle?.dsseEnvelope?.payload ?? "", "base64").toString("utf8"),
  );
  const sha512 = Buffer.from(item.integrity.slice("sha512-".length), "base64").toString("hex");
  const purl = `pkg:npm/${item.name.replace(/^@/u, "%40")}@${item.version}`;
  if (
    payload.predicateType !== "https://slsa.dev/provenance/v1" ||
    !payload.subject?.some(
      ({ name, digest }) => name === purl && digest?.sha512 === sha512,
    )
  ) {
    throw new Error(`${spec} provenance does not identify the reviewed archive`);
  }
  const workflow = payload.predicate?.buildDefinition?.externalParameters?.workflow;
  const dependencies = payload.predicate?.buildDefinition?.resolvedDependencies ?? [];
  if (
    workflow?.repository !== "https://github.com/aexhq/brain" ||
    workflow?.path !== ".github/workflows/npm-publish.yml" ||
    workflow?.ref !== `refs/tags/release/sha-${expectedCommit}` ||
    !dependencies.some(({ digest }) => digest?.gitCommit === expectedCommit)
  ) {
    throw new Error(`${spec} provenance does not bind the selected Brain commit and workflow`);
  }
};

const verifyRegistrySignatures = () => {
  const auditDirectory = mkdtempSync(path.join(os.tmpdir(), "brain-npm-audit-"));
  try {
    writeFileSync(
      path.join(auditDirectory, "package.json"),
      `${JSON.stringify({
        name: "brain-release-audit",
        private: true,
        dependencies: Object.fromEntries(
          manifest.packages.map(({ name, version }) => [name, version]),
        ),
      })}\n`,
    );
    run(["install", "--ignore-scripts", "--no-audit", "--no-fund"], "inherit", auditDirectory);
    // npm verifies the registry signature and the Sigstore provenance attestation for every
    // downloaded release object. The explicit payload checks above bind that proof to this SHA.
    run(["audit", "signatures"], "inherit", auditDirectory);
  } finally {
    rmSync(auditDirectory, { recursive: true, force: true });
  }
};

const verifyPublishedRelease = async () => {
  for (const item of manifest.packages) {
    assertRegistryObject(item);
    await assertOriginalProvenance(item);
  }
  verifyRegistrySignatures();
};

const operation = process.argv[2];
if (operation === "stage") {
  const existingIntegrities = new Map();
  // Fail before the first mutation if any immutable version collides.
  for (const item of manifest.packages) {
    const spec = `${item.name}@${item.version}`;
    const existing = registryValue(spec, "dist.integrity");
    if (existing !== undefined && existing !== item.integrity) {
      throw new Error(`${spec} is immutable and already has a different registry integrity`);
    }
    existingIntegrities.set(spec, existing);
  }
  for (const item of manifest.packages) {
    const spec = `${item.name}@${item.version}`;
    if (existingIntegrities.get(spec) === undefined) {
      run(
        [
          "publish",
          path.join(directory, item.filename),
          "--access",
          "public",
          "--tag",
          "next",
          "--provenance",
        ],
        "inherit",
      );
    }
    await waitFor(() => registryValue(spec, "dist.integrity"), item.integrity, `${spec} integrity`);
    await waitFor(
      () => registryValue(spec, "dist.attestations.provenance.predicateType"),
      "https://slsa.dev/provenance/v1",
      `${spec} provenance`,
    );
    await waitFor(
      () => registryValue(`${item.name}@next`, "version"),
      item.version,
      `${item.name}@next`,
    );
    process.stdout.write(`staged ${spec} (${item.integrity})\n`);
  }
  await verifyPublishedRelease();
} else if (operation === "promote") {
  if (!process.env.NODE_AUTH_TOKEN) {
    throw new Error("the protected npm-production environment has no NPM_DIST_TAG_TOKEN");
  }
  // Validate every immutable object and its original OIDC provenance before moving any tag.
  for (const item of manifest.packages) {
    const staged = registryValue(`${item.name}@next`, "version");
    if (staged !== item.version) {
      throw new Error(
        `${item.name}@next is ${staged ?? "absent"}; refusing to promote ${item.name}@${item.version}`,
      );
    }
  }
  await verifyPublishedRelease();
  for (const item of manifest.packages) {
    const spec = `${item.name}@${item.version}`;
    run(["dist-tag", "add", spec, "latest"], "inherit");
    await waitFor(
      () => registryValue(`${item.name}@latest`, "version"),
      item.version,
      `${item.name}@latest`,
    );
    process.stdout.write(`promoted ${spec} without republishing\n`);
  }
} else {
  throw new Error("usage: publish.mjs stage|promote");
}
