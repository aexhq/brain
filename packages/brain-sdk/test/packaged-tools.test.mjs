import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { createInterface } from "node:readline";
import { mkdtemp, mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { build } from "esbuild";
import { environment, inspectTool } from "../dist/index.js";
import { loadHostTool } from "../dist/extensions.js";
import { runTool } from "../dist/runtime.js";

const sdk = fileURLToPath(new URL("..", import.meta.url));
const npm = (cwd, ...args) => execFileSync(process.execPath, [process.env.npm_execpath, ...args], { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
const pack = (cwd, destination) => JSON.parse(npm(cwd, "pack", "--ignore-scripts", "--json", "--pack-destination", destination))[0].filename;

test("ordinary compilation publishes browser-safe bindings and an executable package with native parsing", { timeout: 120_000 }, async t => {
  const root = await mkdtemp(join(tmpdir(), "brain-package-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const archive = join(root, pack(sdk, root));
  const library = join(root, "library");
  const app = join(root, "app");
  await Promise.all([mkdir(library), mkdir(app)]);
  await writeFile(join(library, "package.json"), JSON.stringify({
    name: "@fixture/files", version: "1.2.3", type: "module", files: ["dist"],
    exports: { ".": { import: "./dist/client.js", types: "./dist/client.d.ts" }, "./runtime": { import: "./dist/runtime.js" } },
    brain: { tools: { ".": { runtime: "./runtime", exports: ["readText"] } } },
    dependencies: { "@aexhq/brain": `file:${archive.replaceAll("\\", "/")}`, zod: "4.4.3" },
  }));
  npm(library, "install", "--ignore-scripts", "--offline", "--no-audit", "--no-fund");
  await writeFile(join(library, "runtime.ts"), `import { readFile } from "node:fs/promises";
import { tool } from "@aexhq/brain";
import { z } from "zod";
export const readText = tool({ name: "read_text", description: "Read a file.",
  options: z.object({ prefix: z.string().transform(text => text.toUpperCase()) }),
  input: z.object({ path: z.string(), suffix: z.string().default("!") }), output: z.string(),
  run: async ({ path, suffix }, ctx) => {
    await Promise.all(["one", "two"].map(text => ctx.model({ messages: [{ role: "user", content: [{ type: "text", text }] }] })));
    await ctx.emitResult("reading", { content: "Reading the file." });
    return ctx.finish(ctx.options.prefix + await readFile(path, "utf8") + suffix);
  },
});`);
  const compiler = fileURLToPath(import.meta.resolve("typescript/lib/tsc.js"));
  execFileSync(process.execPath, [compiler, "runtime.ts", "--module", "NodeNext", "--target", "ES2023", "--declaration", "--strict", "--skipLibCheck", "--outDir", "dist", "--typeRoots", resolve(sdk, "../../node_modules/@types")], { cwd: library, stdio: "pipe" });
  execFileSync(process.execPath, [join(library, "node_modules/@aexhq/brain/bin/brain-tools.mjs")], { cwd: library, stdio: "pipe" });
  const published = join(root, pack(library, root));
  await writeFile(join(app, "package.json"), JSON.stringify({ type: "module", dependencies: { "@fixture/files": `file:${published.replaceAll("\\", "/")}` } }));
  npm(app, "install", "--ignore-scripts", "--offline", "--no-audit", "--no-fund");
  // Only the published archive supplies the executable and its dependencies.
  await rm(library, { recursive: true, force: true });
  const clientFile = join(app, "node_modules/@fixture/files/dist/client.js");
  const browser = await build({ entryPoints: [clientFile], bundle: true, platform: "browser", format: "esm", write: false, metafile: true });
  assert.ok(!Object.keys(browser.metafile.inputs).some(name => name.endsWith("/dist/runtime.js")));
  const { readText } = await import(pathToFileURL(clientFile).href);
  const remote = environment({ url: () => "https://workspace.example" })({ name: "workspace" });
  const source = inspectTool(readText({ env: remote, prefix: "hello " }));
  assert.equal(source.handler, undefined);
  assert.deepEqual(source.implementation, { type: "node_package", package: "@fixture/files", version: "1.2.3", entry: "./runtime", export: "readText", configuration: { prefix: "hello " } });
  const inputFile = join(app, "input.txt");
  await writeFile(inputFile, "world");
  const updates = [];
  const frame = { sessionId: "session", environment: "workspace", sequence: 1, arguments: { path: inputFile },
    emit: async () => 1, model: async () => ({ message: { role: "assistant", content: [] }, stop_reason: "end_turn", usage: {} }),
    update: async value => { updates.push(value); return updates.length; } };
  await runTool(source.implementation, app, frame);
  assert.deepEqual(updates, [
    { type: "result", outcome: { status: "ok", value: "reading", content: "Reading the file." } },
    { type: "finish", outcome: { status: "ok", value: "HELLO world!" } },
  ]);
  const local = inspectTool(await loadHostTool(readText({ prefix: "local " })));
  assert.equal(typeof local.handler, "function");
  assert.equal(local.configuration.prefix, "LOCAL ");
  const processTool = spawn(process.execPath, [join(app, "node_modules/@aexhq/brain/bin/brain-tool-runtime.mjs"), app], { stdio: ["pipe", "pipe", "pipe"] });
  const exit = once(processTool, "exit");
  let errors = "";
  processTool.stderr.on("data", chunk => { errors += chunk; });
  const services = [];
  let receipt;
  const lines = createInterface({ input: processTool.stdout });
  processTool.stdin.write(JSON.stringify({ implementation: source.implementation, input: frame.arguments,
    sessionId: frame.sessionId, environment: frame.environment, sequence: frame.sequence }) + "\n");
  for await (const line of lines) {
    const value = JSON.parse(line);
    if (value.type !== "service") { receipt = value; continue; }
    services.push(value);
    processTool.stdin.write(JSON.stringify({ id: value.id, output: value.method === "model" ? await frame.model() : services.length }) + "\n");
  }
  assert.equal((await exit)[0], 0, errors);
  assert.deepEqual(receipt, { type: "returned" });
  assert.deepEqual(services.map(value => value.method), ["model", "model", "result", "finish"]);
  assert.equal(services.at(-1).input.value, "HELLO world!");
  await assert.rejects(runTool({ ...source.implementation, version: "2.0.0" }, app, frame), /needs @fixture\/files@2.0.0/u);
  const declaration = await readFile(join(app, "node_modules/@fixture/files/dist/client.d.ts"), "utf8");
  assert.match(declaration, /typeof import\("\.\/runtime.js"\)\["readText"\]/u);
});
