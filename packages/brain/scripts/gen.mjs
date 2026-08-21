import { compile } from "json-schema-to-typescript";
import openapiTS, { astToString } from "openapi-typescript";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const contracts = path.join(root, "contracts");
const generated = path.join(here, "../src/generated");
await mkdir(generated, { recursive: true });

const schemaPath = path.join(contracts, "session/v1/schemas.json");
const schema = JSON.parse(await readFile(schemaPath, "utf8"));
const banner = `/* eslint-disable */\n/** GENERATED from Brain-owned contracts/session/v1. DO NOT EDIT. */\n`;
const session = await compile(schema, "BrainSessionAPI", {
  bannerComment: banner,
  additionalProperties: false,
  strictIndexSignatures: true,
  unreachableDefinitions: true,
  style: { singleQuote: false, printWidth: 100 },
  cwd: path.dirname(schemaPath),
});
await writeFile(path.join(generated, "session.ts"), session.replace(/\r\n/gu, "\n"));

const handSchemaPath = path.join(contracts, "hand/contract.json");
const handSchema = JSON.parse(await readFile(handSchemaPath, "utf8"));
const hand = await compile(handSchema, "BrainHandContract", {
  bannerComment: banner,
  additionalProperties: false,
  strictIndexSignatures: true,
  unreachableDefinitions: true,
  style: { singleQuote: false, printWidth: 100 },
  cwd: path.dirname(handSchemaPath),
});
await writeFile(path.join(generated, "hand.ts"), hand.replace(/\r\n/gu, "\n"));

const openapiPath = path.join(contracts, "session/v1/openapi.yaml");
const paths = await openapiTS(pathToFileURL(openapiPath), {
  exportType: true,
  // Server-side serde defaults remain optional request fields on the public client surface.
  defaultNonNullable: false,
});
await writeFile(path.join(generated, "paths.ts"), banner + astToString(paths));
await writeFile(
  path.join(here, "../schemas/session.v1.json"),
  await readFile(schemaPath),
);
