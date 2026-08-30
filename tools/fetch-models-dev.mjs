#!/usr/bin/env node
// Refreshes the vendored models.dev snapshot the provider catalog is generated
// from. Deliberately manual — no cron, no PR bot: a refresh is a reviewed
// change that rides the normal release train. After running this, run
// `npm run gen` to regenerate the catalog and commit the whole diff.

import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SNAPSHOT = join(ROOT, "catalog", "models-dev", "api.json");
const DIGEST = join(ROOT, "catalog", "models-dev", "api.digest");
const SOURCE = "https://models.dev/api.json";

const response = await fetch(SOURCE);
if (!response.ok) {
  console.error(`${SOURCE} answered ${response.status}`);
  process.exit(1);
}
// Canonicalize: sorted keys, stable formatting, one trailing newline — so a
// refresh diffs by content, never by upstream serialization order.
const canonical = `${JSON.stringify(sortKeys(await response.json()), null, 1)}\n`;
const digest = createHash("sha256").update(canonical).digest("hex");

let previous;
try {
  previous = readFileSync(DIGEST, "utf-8").trim();
} catch {
  previous = "";
}
if (previous === digest) {
  console.log(`snapshot unchanged (${digest})`);
  process.exit(0);
}
mkdirSync(dirname(SNAPSHOT), { recursive: true });
writeFileSync(SNAPSHOT, canonical);
writeFileSync(DIGEST, `${digest}\n`);
console.log(`snapshot refreshed (${previous || "none"} -> ${digest}); now run: npm run gen`);

function sortKeys(value) {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, sortKeys(value[key])]),
    );
  }
  return value;
}
