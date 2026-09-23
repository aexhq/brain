import { createRequire } from "node:module";
import { readFile } from "node:fs/promises";
import { join, resolve, relative } from "node:path";
import { pathToFileURL } from "node:url";
import { z } from "zod";
import { inspectTool } from "./extensions.js";
import { HostToolRegistry, type InvokeFrame } from "./host.js";

const packageName = /^(?:@[a-z0-9][a-z0-9._-]*\/)?[a-z0-9][a-z0-9._-]*$/u;
export const nodePackage = z.strictObject({
  type: z.literal("node_package"),
  package: z.string().regex(packageName),
  version: z.string().regex(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/u),
  entry: z.string().refine(value => value === "." || /^\.\/[A-Za-z0-9_-][A-Za-z0-9._-]*(?:\/[A-Za-z0-9_-][A-Za-z0-9._-]*)*$/u.test(value), "invalid package export"),
  export: z.string().regex(/^[A-Za-z_$][\w$]*$/u),
  configuration: z.record(z.string(), z.json()).default({}),
});

/** The operator prepares dependencies with its ordinary package manager in an isolated runtime. */
export async function loadTool(implementation: unknown, directory: string): Promise<ReturnType<typeof inspectTool>> {
  const descriptor = nodePackage.parse(implementation);
  const require = createRequire(pathToFileURL(join(resolve(directory), "package.json")));
  const directories = require.resolve.paths(descriptor.package) ?? [];
  let installed;
  let packageDirectory = "";
  for (const modules of directories) {
    try {
      installed = JSON.parse(await readFile(join(modules, descriptor.package, "package.json"), "utf8"));
      packageDirectory = join(modules, descriptor.package);
      break;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
  }
  if (installed?.version !== descriptor.version) throw new Error(`Environment needs ${descriptor.package}@${descriptor.version}`);
  const exported = installed.exports?.[descriptor.entry];
  const target = typeof exported === "string" ? exported : exported?.import ?? exported?.default;
  if (typeof target !== "string" || !target.startsWith("./")) throw new TypeError("package needs an explicit JavaScript runtime export");
  const filename = resolve(packageDirectory, target);
  if (relative(packageDirectory, filename).startsWith("..")) throw new TypeError("runtime export must be inside the package");
  const module = await import(pathToFileURL(filename).href);
  const factory = module[descriptor.export];
  if (typeof factory !== "function") throw new TypeError(`package has no Tool export ${descriptor.export}`);
  const tool = inspectTool(factory(descriptor.configuration));
  if (tool.handler === undefined || tool.contract === undefined) throw new TypeError("packaged executable must define run");
  return tool;
}

/** Execute the ordinary Tool lifecycle through the Environment's invocation services. */
export async function runTool(implementation: unknown, directory: string, frame: Omit<InvokeFrame, "name">): Promise<void> {
  const tool = await loadTool(implementation, directory);
  const registry = new HostToolRegistry();
  registry.register(frame.environment, tool.contract!, tool.handler!);
  await registry.run({ ...frame, name: tool.definition.name });
}
