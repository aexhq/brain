import { readFile, writeFile } from "node:fs/promises";
import { dirname, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { toolMetadata } from "./extensions.js";

/** Run after the package's ordinary compiler. Executable exports are declared in package.json. */
export async function packageTools(directory = process.cwd()): Promise<void> {
  const root = resolve(directory);
  const pkg = JSON.parse(await readFile(resolve(root, "package.json"), "utf8"));
  if (typeof pkg.name !== "string" || typeof pkg.version !== "string" || pkg.brain?.tools === undefined) {
    throw new TypeError("package.json needs name, version and brain.tools");
  }
  for (const [entry, settings] of Object.entries(pkg.brain.tools) as [string, { runtime: string; exports: string[] }][]) {
    const client = pkg.exports?.[entry];
    const executable = pkg.exports?.[settings.runtime];
    const runtime = typeof executable === "string" ? executable : executable?.import;
    if (typeof client?.import !== "string" || typeof client?.types !== "string" || typeof runtime !== "string"
      || !Array.isArray(settings.exports) || settings.exports.length === 0) {
      throw new TypeError(`Tool entry ${entry} needs client import/types, a runtime export, and exported factory names`);
    }
    for (const path of [client.import, client.types, runtime]) {
      if (!path.startsWith("./") || relative(root, resolve(root, path)).startsWith("..")) throw new TypeError("Tool entries must be inside the package");
    }
    const module = await import(pathToFileURL(resolve(root, runtime)).href);
    const runtimePath = relative(dirname(resolve(root, client.import)), resolve(root, runtime)).replaceAll("\\", "/");
    const typesPath = relative(dirname(resolve(root, client.types)), resolve(root, runtime)).replaceAll("\\", "/");
    const code = ['import { bindTool } from "@aexhq/brain";'];
    const types: string[] = [];
    for (const name of settings.exports) {
      if (!/^[A-Za-z_$][\w$]*$/u.test(name)) throw new TypeError("Tool export must be a JavaScript identifier");
      const metadata = toolMetadata(module[name]);
      const implementation = { type: "node_package", package: pkg.name, version: pkg.version, entry: settings.runtime, export: name };
      code.push(`export const ${name} = bindTool(${JSON.stringify(metadata)}, ${JSON.stringify(implementation)}, { url: new URL(${JSON.stringify("./" + runtimePath)}, import.meta.url), export: ${JSON.stringify(name)} });`);
      types.push(`export declare const ${name}: typeof import(${JSON.stringify("./" + typesPath)})[${JSON.stringify(name)}];`);
    }
    await writeFile(resolve(root, client.import), code.join("\n") + "\n");
    await writeFile(resolve(root, client.types), types.join("\n") + "\n");
  }
}
