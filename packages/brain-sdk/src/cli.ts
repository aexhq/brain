#!/usr/bin/env node
import { build } from "./build.js";
import { watch } from "node:fs";
import { resolve } from "node:path";

const args = process.argv.slice(2);
if (args[0] !== "build") usage();
let entry: string | undefined;
let out: string | undefined;
let watching = false;
for (let index = 1; index < args.length; index += 1) {
  const value = args[index]!;
  if (value === "--out") {
    out = args[++index];
    if (out === undefined) usage();
  } else if (value === "--watch") {
    watching = true;
  } else if (value.startsWith("-")) {
    usage();
  } else if (entry === undefined) {
    entry = value;
  } else {
    usage();
  }
}
const options = { ...(entry === undefined ? {} : { entry }), ...(out === undefined ? {} : { out }) };
await runBuild();
if (watching) {
  let running = false;
  let pending = false;
  const source = resolve(entry ?? "src/index.ts");
  const watcher = watch(resolve(source, ".."), { recursive: true }, () => {
    pending = true;
    void rebuild();
  });
  process.stderr.write(`watching ${source}\n`);
  await new Promise<void>((resolvePromise) => {
    const stop = () => { watcher.close(); resolvePromise(); };
    process.once("SIGINT", stop);
    process.once("SIGTERM", stop);
  });

  async function rebuild(): Promise<void> {
    if (running) return;
    running = true;
    while (pending) {
      pending = false;
      try { await runBuild(); }
      catch (error) { process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`); }
    }
    running = false;
  }
}

async function runBuild(): Promise<void> {
  const built = await build(options);
  for (const extension of built) process.stderr.write(`${extension.name} (${extension.kind})${extension.digest === undefined ? "" : ` -> ${extension.digest} (${extension.bytes} bytes)`}\n`);
}

function usage(): never {
  process.stderr.write("usage: brain build [entry] [--out directory] [--watch]\n");
  process.exit(2);
}
