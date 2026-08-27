import { compile } from "json-schema-to-typescript";
import openapiTS, { astToString } from "openapi-typescript";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

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

const openapi = await openapiTS(pathToFileURL(path.join(root, "contracts/session/v1/openapi.yaml")), {
  exportType: true,
  defaultNonNullable: false,
});
await writeFile(path.join(output, "paths.ts"), banner + astToString(openapi));
