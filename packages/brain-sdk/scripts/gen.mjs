import { compile } from "json-schema-to-typescript";
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

// Everything written here is rendered from contracts/, which `cargo run -p brain-contracts`
// renders from the Rust types. Nothing under src/generated or contracts/ is edited by hand.
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const output = path.resolve(here, "../src/generated");
await mkdir(output, { recursive: true });

const schemaPath = path.join(root, "contracts/session/v1/schemas.json");
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

// Component authors compile elsewhere; these contracts define the imports Brain hosts.
const wit = path.resolve(here, "../contracts");
await mkdir(wit, { recursive: true });
await copyFile(path.join(root, "contracts/agentloop/v1/agentloop.wit"), path.join(wit, "agentloop.wit"));
await copyFile(path.join(root, "contracts/tool/v1/tool.wit"), path.join(wit, "tool.wit"));
