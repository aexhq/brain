import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";

// Dependency order: the loop packages build against @aexhq/agentloop, so the SDK publishes
// (and becomes registry-visible) first.
const workspaces = ["brain", "brain-tools", "agentloop", "loop-pi", "loop-codex"];
const root = path.resolve(import.meta.dirname, "..");
const npmCli = [
  process.env.npm_execpath,
  path.join(path.dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js"),
  path.resolve(path.dirname(process.execPath), "../lib/node_modules/npm/bin/npm-cli.js"),
].find((candidate) => candidate !== undefined && existsSync(candidate));
if (npmCli === undefined) throw new Error("could not locate npm-cli.js for the active Node runtime");

const fail = (message) => {
  throw new Error(message);
};

const run = (args) =>
  execFileSync(process.execPath, [npmCli, ...args], {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();

const packageDocument = async (workspace) =>
  JSON.parse(await readFile(path.join(root, "packages", workspace, "package.json"), "utf8"));

const parseManifest = async (filename) => {
  const manifest = JSON.parse(await readFile(filename, "utf8"));
  if (manifest.schema !== 1 || !Array.isArray(manifest.packages)) {
    fail("release manifest has an unsupported shape");
  }
  return manifest;
};

const pack = async (directory) => {
  await mkdir(directory, { recursive: false });
  const packages = [];
  const releaseNames = new Set();
  for (const workspace of workspaces) {
    const document = await packageDocument(workspace);
    if (releaseNames.has(document.name)) fail(`duplicate release package ${document.name}`);
    releaseNames.add(document.name);
    if (document.publishConfig?.tag !== "next") {
      fail(`${document.name} must default publication to the next dist-tag`);
    }
    const packed = JSON.parse(
      run(["pack", "--json", "--workspace", document.name, "--pack-destination", directory]),
    );
    if (!Array.isArray(packed) || packed.length !== 1) {
      fail(`npm pack returned an unexpected result for ${document.name}`);
    }
    const item = packed[0];
    if (item.name !== document.name || item.version !== document.version) {
      fail(`packed identity drifted for ${document.name}`);
    }
    const archive = path.join(directory, item.filename);
    const integrity = `sha512-${createHash("sha512")
      .update(await readFile(archive))
      .digest("base64")}`;
    if (integrity !== item.integrity) fail(`npm reported the wrong integrity for ${document.name}`);
    packages.push({
      workspace,
      name: document.name,
      version: document.version,
      filename: item.filename,
      integrity,
      dependencies: document.dependencies ?? {},
      peerDependencies: document.peerDependencies ?? {},
    });
  }

  for (const item of packages) {
    for (const [name, version] of Object.entries(item.dependencies)) {
      const local = packages.find((candidate) => candidate.name === name);
      if (local !== undefined && version !== local.version) {
        fail(`${item.name} must depend on the exact release version ${name}@${local.version}`);
      }
    }
  }

  const manifest = {
    schema: 1,
    source: process.env.EXPECTED_COMMIT ?? process.env.GITHUB_SHA ?? "local",
    packages,
  };
  await writeFile(path.join(directory, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  // Registry mutation consumes these reviewed copies from the immutable workflow artifact.
  for (const filename of ["npm-release.mjs", "publish.mjs"]) {
    await writeFile(path.join(directory, filename), await readFile(path.join(root, "tools", filename)));
  }
};

const versions = async (filename) => {
  const manifest = await parseManifest(filename);
  process.stdout.write(
    manifest.packages.map(({ name, version }) => `${name}@${version}`).join(","),
  );
};

const markdown = async (filename) => {
  const manifest = await parseManifest(filename);
  process.stdout.write("| package | version | sha512 integrity |\n| --- | --- | --- |\n");
  for (const item of manifest.packages) {
    process.stdout.write(`| \`${item.name}\` | \`${item.version}\` | \`${item.integrity}\` |\n`);
  }
};

const [command, argument] = process.argv.slice(2);
if (command === "pack" && argument !== undefined) await pack(path.resolve(argument));
else if (command === "versions" && argument !== undefined) await versions(path.resolve(argument));
else if (command === "markdown" && argument !== undefined) await markdown(path.resolve(argument));
else fail("usage: npm-release.mjs pack <directory> | versions|markdown <manifest.json>");
