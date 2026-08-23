import { resolve, sep } from "node:path";
import type { ToolContext } from "@aexhq/brain";

export function workspaceOf(context: ToolContext): string {
  if (context.workspace === undefined) throw new Error("This Tool requires Aex-managed execution");
  return context.workspace;
}

export function workspacePath(workspace: string, requested: string): string {
  const root = resolve(workspace);
  const value = resolve(root, requested);
  if (value !== root && !value.startsWith(`${root}${sep}`)) {
    throw new Error(`path escapes the Environment workspace: ${requested}`);
  }
  return value;
}
