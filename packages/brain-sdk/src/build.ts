import { componentize } from "@bytecodealliance/componentize-js";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { build as bundle } from "esbuild";
import { z } from "zod";

import { extensionSource, inspectToolProgram } from "./extensions.js";
import type { Program } from "./types.js";

export const BUILDER_TOOLCHAIN = "brain-build-2 componentize-js-0.19.3";

export interface BuildOptions { readonly entry?: string; readonly out?: string }
export interface BuiltExtension { readonly name: string; readonly kind: "agentloop" | "tool" | "environment"; readonly artifact?: string; readonly identity?: string; readonly bytes?: number; readonly program?: Program["kind"] }

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
  const programs = new Map<string, Program>();
  for (const [name, definition] of definitions) {
    if (!validIdentifier(name)) throw new Error(`extension export ${JSON.stringify(name)} is not a stable identifier`);
    const kind = definition[extensionSource].kind;
    if (kind === "agentloop") {
      const artifact = `${name}.brain.json`;
      const packageValue = await buildAgentloop(entry, name, join(out, artifact));
      built.push({ name, kind, artifact, identity: packageValue.manifest.component_identity, bytes: packageValue.manifest.component_bytes });
    } else if (kind === "tool") {
      const source = definition[extensionSource] as unknown as { contract: { description: string; input: z.ZodType; output?: z.ZodType; needs?: readonly string[]; bindings?: Readonly<Record<string, z.ZodType>> } };
      const programSource = inspectToolProgram(definition);
      const contract = {
        name,
        description: source.contract.description,
        input_schema: z.toJSONSchema(source.contract.input),
        ...(source.contract.output === undefined ? {} : { output_schema: z.toJSONSchema(source.contract.output) }),
      };
      // The program: for esm a self-contained single-file bundle named by its
      // sha-256; for shell and http the script or request template itself, named
      // the same way. Binding values are structurally impossible in any of them —
      // only the names enter the manifest.
      let payload: string;
      let program: Program;
      if (programSource.kind === "esm") {
        payload = await buildProvisionedPayload(entry, name);
        program = { kind: "esm", identity: identityOf(payload) };
        const contractDigest = createHash("sha256").update(canonical(contract)).digest("hex");
        await buildTool(entry, name, join(out, "runtime", `${name}.mjs`), contractDigest);
        toolRegistry[name] = { contract_digest: contractDigest, filename: `${name}.mjs` };
      } else if (programSource.kind === "shell") {
        payload = programSource.script;
        program = { kind: "shell", identity: identityOf(payload), script: programSource.script };
      } else {
        payload = canonical(programSource.request);
        program = { kind: "http", identity: identityOf(payload), request: programSource.request };
      }
      const manifest = {
        ...contract,
        needs: [...(source.contract.needs ?? [])],
        binding_names: Object.keys(source.contract.bindings ?? {}),
        program,
      };
      const artifact = `${name}.tool.json`;
      await writeFile(join(out, artifact), `${JSON.stringify({ manifest, payload })}\n`);
      programs.set(name, program);
      built.push({ name, kind, artifact, identity: program.identity, bytes: Buffer.byteLength(payload), program: program.kind });
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
    const program = programs.get(name);
    const identityArguments = program !== undefined
      ? `, new URL(${JSON.stringify(`./${artifact}`)}, import.meta.url), undefined, ${JSON.stringify(program)}`
      : artifact !== undefined
        ? `, new URL(${JSON.stringify(`./${artifact}`)}, import.meta.url)`
        : runtimeName === undefined ? "" : `, undefined, ${JSON.stringify(runtimeName)}`;
    return `export const ${name} = definitions.${name};\ninstallExtensionIdentity(${name}, ${JSON.stringify(name)}${identityArguments});`;
  }).join("\n");
  const index = `import { installExtensionIdentity } from "@aexhq/brain";\nimport * as definitions from "./source.mjs";\n\n${imports}\n`;
  await writeFile(join(out, "index.mjs"), index);
  return built;
}

function identityOf(payload: string): string {
  return createHash("sha256").update(payload).digest("hex");
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

async function buildProvisionedPayload(entry: string, name: string): Promise<string> {
  const compiled = await bundle({
    stdin: {
      contents: `
import { provisionedToolRuntime } from "@aexhq/brain";
import { ${name} as definition } from ${JSON.stringify(entry.replaceAll("\\", "/"))};
export default provisionedToolRuntime(definition);
`,
      resolveDir: dirname(entry), sourcefile: `${name}.provisioned-entry.js`, loader: "js",
    },
    bundle: true, format: "esm", platform: "node", target: "node22", write: false, legalComments: "none",
  });
  const output = compiled.outputFiles[0];
  if (output === undefined) throw new Error(`esbuild produced no provisioned payload for tool ${name}`);
  return output.text;
}

async function buildAgentloop(entry: string, name: string, out: string): Promise<AgentloopPackage> {
  const compiled = await bundle({
    stdin: { contents: componentWrapper(entry, name), resolveDir: dirname(entry), sourcefile: `${name}.brain-entry.js`, loader: "js" },
    bundle: true, format: "esm", platform: "neutral", write: false, legalComments: "none", external: ["node:*", "aex:agentloop/*"],
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
    manifest: { contract_version: "agentloop/v2", component_identity: createHash("sha256").update(component).digest("hex"), component_bytes: component.byteLength, toolchain: BUILDER_TOOLCHAIN },
    component_base64: Buffer.from(component).toString("base64"),
  };
  await writeFile(out, `${JSON.stringify(packageValue)}\n`);
  return packageValue;
}

interface AgentloopPackage {
  readonly manifest: { readonly contract_version: "agentloop/v2"; readonly component_identity: string; readonly component_bytes: number; readonly toolchain: string };
  readonly component_base64: string;
}

function componentWrapper(entry: string, name: string): string {
  const specifier = relative(dirname(entry), entry).replaceAll("\\", "/");
  const normalized = specifier.startsWith(".") ? specifier : `./${specifier}`;
  return `
import { runTurn } from "@aexhq/brain";
import * as host from "aex:agentloop/host@2.0.0";
import { ${name} as definition } from ${JSON.stringify(normalized)};

// A host import that fails throws a ComponentError whose payload is the turn-error
// record; it is rethrown as an Error the loop can read the code off.
const hosted = (call) => {
  try {
    return call();
  } catch (error) {
    const payload = error !== null && typeof error === "object" && "payload" in error ? error.payload : undefined;
    const failure = new Error(payload?.message ?? String(error?.message ?? error));
    failure.code = payload?.code ?? "host_error";
    failure.retryable = payload?.retryable ?? false;
    throw failure;
  }
};

export async function turn(input) {
  try {
    const output = await runTurn(definition, {
      input: JSON.parse(input.inputJson),
      transcript: JSON.parse(input.transcriptJson),
      slots: JSON.parse(input.slotsJson),
      events: JSON.parse(input.eventsJson),
      configuration: JSON.parse(input.configurationJson),
      system: input.system,
      tools: JSON.parse(input.toolsJson),
      runtime: { logicalTimeMs: input.runtime.logicalTimeMs },
    }, {
      model: (requestJson) => hosted(() => host.model(requestJson)),
      dispatch: (callsJson) => hosted(() => host.dispatch(callsJson)),
      append: (kind, payloadJson) => hosted(() => host.append(kind, payloadJson)),
      telemetry: (recordJson) => host.telemetry(recordJson),
    });
    return {
      transcriptJson: JSON.stringify(output.transcript),
      slotsJson: JSON.stringify(output.slots),
      resultJson: output.result === undefined ? undefined : JSON.stringify(output.result),
    };
  } catch (error) {
    // Thrown with a payload so the component answers with the contract's error variant
    // instead of trapping.
    const failure = new Error(String(error?.message ?? error));
    failure.payload = {
      code: typeof error?.code === "string" && error.code.length > 0 ? error.code : "agentloop_failed",
      message: String(error?.message ?? error) || "Agentloop turn failed",
      retryable: Boolean(error?.retryable),
    };
    throw failure;
  }
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
