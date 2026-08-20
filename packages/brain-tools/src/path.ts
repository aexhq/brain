import { resolve, sep } from "node:path";

export function workspacePath(workspace: string, requested: string): string {
  const root = resolve(workspace);
  const value = resolve(root, requested);
  if (value !== root && !value.startsWith(`${root}${sep}`)) {
    throw new Error(`path escapes the Hand workspace: ${requested}`);
  }
  return value;
}
