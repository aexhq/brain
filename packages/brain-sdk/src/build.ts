import { componentize } from "@bytecodealliance/componentize-js";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { build as bundle } from "esbuild";
import { z } from "zod";

import { extensionSource } from "./extensions.js";

export const BUILDER_TOOLCHAIN = "brain-build-1 componentize-js-0.19.3";

export interface BuildOptions { readonly entry?: string; readonly out?: string }
export interface BuiltExtension { readonly name: string; readonly kind: "agentloop" | "tool" | "environment"; readonly artifact?: string; readonly identity?: string; readonly bytes?: number }

export async function build(options: BuildOptions = {}): Promise<readonly BuiltExtension[]> {
  const entry = resolve(options.entry ?? "src/index.ts");
  const out = resolve(options.out ?? "dist");
  await mkdir(out, { recursive: true });
  if (/\.(?:ts|tsx|mts|cts)$/u.test(entry)) await emitDeclarations(entry, out);
  const sourcePath = join(out, "source.mjs");
  await bundle({ entryPoints: [entry], outfile: sourcePath, bundle: true, format: "esm", platform: "node", target: "node22", legalComments: "none" });
  const loaded = await import(`${pathToFileURL(sourcePath).href}?build=${Date.now()}`) as Record<string, unknown>;
  const definitions = Object.entries(loaded).filter((entry): entry is [string, Function & { [extensionSource]: { kind: "agentloop" | "tool" | "environment" } }] =>
    typeof entry[1] === "function" && (entry[1] as Function & { [extensionSource]?: unknown })[extensionSource] !== undefined,
  );
  if (definitions.length === 0) throw new Error(`${entry} exports no agentloop, tool, or environment extensions`);
  const built: BuiltExtension[] = [];
  const toolRegistry: Record<string, { readonly contract_digest: string; readonly filename: string }> = {};
  const environmentRuntimeNames = new Map<string, string>();
  for (const [name, definition] of definitions) {
    if (!validIdentifier(name)) throw new Error(`extension export ${JSON.stringify(name)} is not a stable identifier`);
    const kind = definition[extensionSource].kind;
    if (kind === "agentloop") {
      const artifact = `${name}.brain.json`;
      const packageValue = await buildAgentloop(entry, name, join(out, artifact));
      built.push({ name, kind, artifact, identity: packageValue.manifest.component_identity, bytes: packageValue.manifest.component_bytes });
    } else if (kind === "tool") {
      const source = definition[extensionSource] as unknown as { contract: { description: string; input: z.ZodType; output?: z.ZodType } };
      const contract = {
        name,
        description: source.contract.description,
        input_schema: z.toJSONSchema(source.contract.input),
        ...(source.contract.output === undefined ? {} : { output_schema: z.toJSONSchema(source.contract.output) }),
      };
      const contractDigest = createHash("sha256").update(canonical(contract)).digest("hex");
      await buildTool(entry, name, join(out, "runtime", `${name}.mjs`), contractDigest);
      toolRegistry[name] = { contract_digest: contractDigest, filename: `${name}.mjs` };
      built.push({ name, kind });
    } else {
      environmentRuntimeNames.set(name, await packageRuntimeName(entry));
      await buildEnvironment(entry, name, join(out, "runtime", `${name}.mjs`));
      built.push({ name, kind });
    }
  }
  if (Object.keys(toolRegistry).length > 0) {
    await mkdir(join(out, "runtime"), { recursive: true });
    await writeFile(join(out, "runtime", "registry.json"), `${JSON.stringify(toolRegistry, null, 2)}\n`);
  }
  await bundle({ entryPoints: [entry], outfile: sourcePath, bundle: true, format: "esm", platform: "node", target: "node22", legalComments: "none", external: ["@aexhq/brain"] });
  const imports = built.map(({ name, artifact }) => {
    const runtimeName = environmentRuntimeNames.get(name);
    const identityArguments = artifact !== undefined
      ? `, new URL(${JSON.stringify(`./${artifact}`)}, import.meta.url)`
      : runtimeName === undefined ? "" : `, undefined, ${JSON.stringify(runtimeName)}`;
    return `export const ${name} = definitions.${name};\ninstallExtensionIdentity(${name}, ${JSON.stringify(name)}${identityArguments});`;
  }).join("\n");
  const index = `import { installExtensionIdentity } from "@aexhq/brain";\nimport * as definitions from "./source.mjs";\n\n${imports}\n`;
  await writeFile(join(out, "index.mjs"), index);
  return built;
}

async function emitDeclarations(entry: string, out: string): Promise<void> {
  const compiler = fileURLToPath(import.meta.resolve("typescript/bin/tsc"));
  const args = [
    compiler,
    entry,
    "--declaration", "--emitDeclarationOnly", "--skipLibCheck",
    "--target", "ES2022", "--module", "NodeNext", "--moduleResolution", "NodeNext",
    "--rootDir", dirname(entry), "--outDir", out,
  ];
  await new Promise<void>((resolvePromise, reject) => {
    const child = spawn(process.execPath, args, { stdio: "inherit" });
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (code === 0) resolvePromise();
      else reject(new Error(`TypeScript declaration build failed${signal === null ? ` with exit ${code}` : ` from signal ${signal}`}`));
    });
  });
}

async function buildEnvironment(entry: string, name: string, out: string): Promise<void> {
  await mkdir(dirname(out), { recursive: true });
  await bundle({
    stdin: {
      contents: `
import { createEnvironmentHandler } from "@aexhq/brain";
import { ${name} as definition } from ${JSON.stringify(entry.replaceAll("\\", "/"))};
export const handle = createEnvironmentHandler(definition);
export default handle;
`,
      resolveDir: dirname(entry), sourcefile: `${name}.environment-entry.js`, loader: "js",
    },
    bundle: true, format: "esm", platform: "node", target: "node22", outfile: out, legalComments: "none",
  });
}

async function buildTool(entry: string, name: string, out: string, contractDigest: string): Promise<void> {
  await mkdir(dirname(out), { recursive: true });
  await bundle({
    stdin: {
      contents: `
import { executeTool } from "@aexhq/brain";
import { ${name} as definition } from ${JSON.stringify(entry.replaceAll("\\", "/"))};
export default {
  kind: "brain.tool-runtime",
  name: ${JSON.stringify(name)},
  contractDigest: ${JSON.stringify(contractDigest)},
  requiredEnv: [],
  execute(input, context) { return executeTool(definition, context.options, input, context); },
};
`,
      resolveDir: dirname(entry), sourcefile: `${name}.tool-entry.js`, loader: "js",
    },
    bundle: true, format: "esm", platform: "node", target: "node22", outfile: out, legalComments: "none",
  });
}

async function buildAgentloop(entry: string, name: string, out: string): Promise<AgentloopPackage> {
  const compiled = await bundle({
    stdin: { contents: componentWrapper(entry, name), resolveDir: dirname(entry), sourcefile: `${name}.brain-entry.js`, loader: "js" },
    bundle: true, format: "esm", platform: "neutral", write: false, legalComments: "none", external: ["node:*"],
  });
  const output = compiled.outputFiles[0];
  if (output === undefined) throw new Error(`esbuild produced no output for Agentloop ${name}`);
  const wit = await readFile(new URL("../contracts/agentloop.wit", import.meta.url), "utf8");
  const work = await mkdtemp(join(tmpdir(), "brain-build-"));
  let component: Uint8Array;
  try {
    const sourcePath = join(work, "brain.js");
    const witPath = join(work, "agentloop.wit");
    await Promise.all([writeFile(sourcePath, output.text), writeFile(witPath, wit)]);
    const compiledComponent = await componentize({ sourcePath, witPath, worldName: "agentloop", disableFeatures: ["stdio", "random", "clocks", "http", "fetch-event"] });
    component = new Uint8Array(compiledComponent.component);
  } finally {
    await rm(work, { recursive: true, force: true });
  }
  const packageValue: AgentloopPackage = {
    manifest: { contract_version: "agentloop/v1", component_identity: createHash("sha256").update(component).digest("hex"), component_bytes: component.byteLength, toolchain: BUILDER_TOOLCHAIN },
    component_base64: Buffer.from(component).toString("base64"),
  };
  await writeFile(out, `${JSON.stringify(packageValue)}\n`);
  return packageValue;
}

interface AgentloopPackage {
  readonly manifest: { readonly contract_version: "agentloop/v1"; readonly component_identity: string; readonly component_bytes: number; readonly toolchain: string };
  readonly component_base64: string;
}

function componentWrapper(entry: string, name: string): string {
  const specifier = relative(dirname(entry), entry).replaceAll("\\", "/");
  const normalized = specifier.startsWith(".") ? specifier : `./${specifier}`;
  return `
import { activateAgentloop } from "@aexhq/brain";
import { ${name} as definition } from ${JSON.stringify(normalized)};

const decodeObservation = (observation) => {
  switch (observation.tag) {
    case "session-started": return { type: "session_started" };
    case "user-message": return { type: "user_message", input: JSON.parse(observation.val) };
    case "model-completed": return { type: "model_completed", response: JSON.parse(observation.val) };
    case "tools-completed": return { type: "tools_completed", results: JSON.parse(observation.val) };
    case "emitted": return { type: "emitted", event: JSON.parse(observation.val) };
    case "cancelled": return { type: "cancelled" };
    default: throw new Error("unknown observation " + observation.tag);
  }
};
const encodeDecision = (decision) => {
  switch (decision.type) {
    case "model": return { tag: "model", val: JSON.stringify(decision.request) };
    case "tools": return { tag: "tools", val: decision.calls.map((call) => ({ callId: call.callId, name: call.name, inputJson: JSON.stringify(call.input) })) };
    case "emit": return { tag: "emit", val: JSON.stringify(decision.event) };
    case "finish": return { tag: "finish", val: decision.result === undefined ? undefined : JSON.stringify(decision.result) };
    case "fail": return { tag: "fail", val: [decision.code, decision.message, decision.retryable] };
    default: throw new Error("unknown Agentloop action " + decision.type);
  }
};
export function step(input) {
  const output = activateAgentloop(definition, {
    context: { state: input.context.stateJson === undefined ? undefined : JSON.parse(input.context.stateJson) },
    observation: decodeObservation(input.observation),
    configuration: JSON.parse(input.configurationJson),
    runtime: { logicalTimeMs: input.runtime.logicalTimeMs },
  });
  return {
    context: { protocolVersion: output.context.protocolVersion, itemsJson: JSON.stringify(output.context.items), stateJson: JSON.stringify(output.context.state) },
    decision: encodeDecision(output.decision),
  };
}
`;
}

function validIdentifier(value: string): boolean { return /^[A-Za-z_$][A-Za-z0-9_$]*$/u.test(value) && value.length <= 128; }

async function packageRuntimeName(entry: string): Promise<string> {
  let directory = dirname(entry);
  for (;;) {
    try {
      const manifest = JSON.parse(await readFile(join(directory, "package.json"), "utf8")) as { name?: unknown };
      if (typeof manifest.name !== "string") throw new Error(`package.json next to ${entry} has no name`);
      const leaf = manifest.name.includes("/") ? manifest.name.slice(manifest.name.lastIndexOf("/") + 1) : manifest.name;
      return leaf.startsWith("env-") ? leaf.slice(4) : leaf;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
    const parent = dirname(directory);
    if (parent === directory) throw new Error(`could not find package.json for ${entry}`);
    directory = parent;
  }
}

function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical((value as Record<string, unknown>)[key])}`).join(",")}}`;
  return JSON.stringify(value);
}
