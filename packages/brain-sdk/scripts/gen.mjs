import { compile } from "json-schema-to-typescript";
import { copyFile, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

// Everything written here is rendered from the crates' `generated/contract/` directories,
// which each crate's `cargo run -p <crate> --bin <crate>-contract` renders from its Rust types.
// Nothing under src/generated or contracts/ is edited by hand.
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const output = path.resolve(here, "../src/generated");
await mkdir(output, { recursive: true });

const schemaPath = path.join(root, "crates/brain-protocol/generated/contract/session/v1/schemas.json");
const schema = JSON.parse(await readFile(schemaPath, "utf8"));
const banner = "/* eslint-disable */\n/** Generated from Brain-owned v1 contracts. Do not edit. */\n";
const session = await compile(schema, "BrainSessionContract", {
  bannerComment: banner,
  additionalProperties: false,
  strictIndexSignatures: true,
  unreachableDefinitions: true,
  style: { singleQuote: false, printWidth: 100 },
  cwd: path.dirname(schemaPath),
});
await writeFile(path.join(output, "session.ts"), session.replace(/\r\n/gu, "\n"));

// Runtime validation uses the same definitions as the wire types, without bundling the session API.
const environmentDefinitions = {};
function includeDefinition(name) {
  if (Object.hasOwn(environmentDefinitions, name)) return;
  const definition = schema.$defs[name];
  if (definition === undefined) throw new Error(`missing contract definition ${name}`);
  environmentDefinitions[name] = definition;
  function visit(value) {
    if (value === null || typeof value !== "object") return;
    if (typeof value.$ref === "string" && value.$ref.startsWith("#/$defs/")) includeDefinition(value.$ref.slice(8));
    for (const child of Object.values(value)) visit(child);
  }
  visit(definition);
}
for (const name of ["EnvironmentCommand", "EnvironmentResponse", "HarnessInitialize", "HarnessSession", "HarnessRun"]) includeDefinition(name);
await writeFile(path.join(output, "environment-schema.ts"),
  `${banner}import type { JSONSchema } from "zod/v4/core";\nexport const environmentDefinitions: Record<string, JSONSchema.JSONSchema> = ${JSON.stringify(environmentDefinitions, null, 2)};\n`);

// The known-provider union, from the providers contract the brain crate renders.
const providersPath = path.join(root, "crates/brain/generated/contract/providers/v1/providers.json");
const { providers } = JSON.parse(await readFile(providersPath, "utf8"));
const providersTs = [
  "// Generated from crates/brain/generated/contract/providers/v1/providers.json.",
  "// Do not edit; refresh with tools/fetch-models-dev.mjs and `npm run gen`.",
  "",
  "export type KnownProviderId =",
  ...providers.map((name) => `  | ${JSON.stringify(name)}`),
  ";",
  "",
  "export const knownProviders: readonly KnownProviderId[] = [",
  ...providers.map((name) => `  ${JSON.stringify(name)},`),
  "];",
  "",
].join("\n");
await writeFile(path.join(output, "providers.ts"), providersTs);

// Component authors compile elsewhere; these contracts define the imports Brain hosts.
const wit = path.resolve(here, "../contracts");
await mkdir(wit, { recursive: true });
const loophostWit = path.join(root, "crates/brain-env/wit");
await rm(path.join(wit, "agentloop.wit"), { force: true });
await copyFile(path.join(loophostWit, "tool/tool.wit"), path.join(wit, "tool.wit"));
await copyFile(schemaPath, path.join(wit, "session.json"));
