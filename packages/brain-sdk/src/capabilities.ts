import type { CapabilityName } from "./types.js";

/**
 * The tool-facing half of the capability contract: typed handles a provisioned
 * tool receives for exactly the capabilities it declares in `requires`, and the
 * environment-facing grant shapes providers enforce behind them.
 *
 * Handles surface failures as `CapabilityError`s. Timeouts and cancellation
 * arrive through the tool's own `signal`/`deadline`, not per-handle knobs, and
 * grant policy is enforced behind the handle — a tool cannot observe or bypass
 * it, only hit it as an error.
 */

/** A typed failure thrown by a capability handle. `code` is an identifier the
 * invoke outcome carries verbatim (for example `path_escape`). */
export class CapabilityError extends Error {
  override readonly name = "CapabilityError";
  constructor(
    public readonly capability: CapabilityName,
    public readonly code: string,
    message: string,
    public readonly details?: unknown,
  ) {
    super(message);
  }
}

export interface ExecOptions {
  readonly cwd?: string;
  readonly env?: Readonly<Record<string, string>>;
  readonly stdin?: string;
  readonly timeoutMs?: number;
}
export interface ExecResult { readonly exitCode: number; readonly stdout: string; readonly stderr: string }
export interface ExecHandle { run(command: string, opts?: ExecOptions): Promise<ExecResult> }

export interface FsEntry { readonly name: string; readonly kind: "file" | "dir" }
export interface FsHandle {
  read(path: string): Promise<Uint8Array>;
  write(path: string, data: Uint8Array | string): Promise<void>;
  list(path: string): Promise<readonly FsEntry[]>;
}

export interface NetFetchRequest {
  readonly url: string;
  readonly method?: string;
  readonly headers?: Readonly<Record<string, string>>;
  readonly body?: string;
}
export interface NetFetchResponse { readonly status: number; readonly headers: Readonly<Record<string, string>>; readonly body: string }
export interface NetHandle { fetch(input: NetFetchRequest): Promise<NetFetchResponse> }

export interface JsHandle { evaluate(source: string, args?: readonly unknown[]): Promise<unknown> }

/** The contract leaves the trusted-input action shape open in v1; ref-based
 * `act()` is a reserved later interface addition. */
export type PageInput = unknown;
export interface ConsoleEntry { readonly level: string; readonly text: string }
export interface PageHandle {
  navigate(url: string, wait?: "none" | "interaction" | "complete"): Promise<void>;
  screenshot(): Promise<Uint8Array>;
  input(action: PageInput): Promise<void>;
  consoleSince(cursor: number): Promise<{ readonly entries: readonly ConsoleEntry[]; readonly cursor: number }>;
}

/** Every capability's tool-facing handle, keyed by its contract name. A tool's
 * run context picks from this by its declared `requires`. */
export interface CapabilityHandles {
  readonly exec: ExecHandle;
  readonly fs: FsHandle;
  readonly net: NetHandle;
  readonly js: JsHandle;
  readonly page: PageHandle;
}

/** Per-capability policy carried on attach (`environment/v2` GrantSet, wire
 * field names). Providers enforce it; tools never see it. */
export interface ExecGrant { readonly timeout_ms_max?: number; readonly output_bytes_max?: number }
export interface FsGrant { readonly root: string }
export interface NetGrant { readonly allow: readonly string[] }
export interface GrantSet {
  readonly exec?: ExecGrant;
  readonly fs?: FsGrant;
  readonly net?: NetGrant;
  readonly js?: Record<string, never>;
  readonly page?: Record<string, never>;
}

/** What an environment registers per capability: given its open instance and
 * the attachment's grants, return the provider — the same interface the tool's
 * handle projects, so the two sides cannot drift. */
export type CapabilityProviderFactory<Instance, Name extends CapabilityName> = (
  context: { readonly instance: Instance; readonly grants: GrantSet },
) => CapabilityHandles[Name];

function clampExec(opts: ExecOptions | undefined, grant: ExecGrant | undefined): ExecOptions {
  const maximum = grant?.timeout_ms_max;
  if (maximum === undefined) return opts ?? {};
  const requested = opts?.timeoutMs;
  return { ...opts, timeoutMs: requested === undefined ? maximum : Math.min(requested, maximum) };
}

/** Pure posix-style normalization (no filesystem, no cwd): resolves `.`/`..`
 * segments so escapes are judged on the resolved path, not its spelling. */
function normalizedPath(path: string): { readonly absolute: boolean; readonly value: string } {
  const absolute = path.startsWith("/");
  const parts: string[] = [];
  for (const part of path.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      if (parts.length > 0 && parts[parts.length - 1] !== "..") parts.pop();
      else if (!absolute) parts.push("..");
    } else {
      parts.push(part);
    }
  }
  return { absolute, value: `${absolute ? "/" : ""}${parts.join("/")}` };
}

function clampPath(root: string, path: string): string {
  const normalizedRoot = normalizedPath(root.replaceAll("\\", "/")).value;
  const requested = path.replaceAll("\\", "/");
  const candidate = normalizedPath(requested.startsWith("/") ? requested : `${normalizedRoot}/${requested}`).value;
  const prefix = normalizedRoot === "/" ? "/" : `${normalizedRoot}/`;
  if (candidate !== normalizedRoot && !candidate.startsWith(prefix)) {
    throw new CapabilityError("fs", "path_escape", `path ${path} escapes the granted root ${root}`);
  }
  return candidate;
}

/**
 * The shared grant-clamping helper for providers. Calling `clamp(opts,
 * grants.exec)` bounds exec options (a requested timeout never exceeds the
 * granted maximum; an absent request gets the maximum); `clamp.path(root,
 * path)` confines an fs path to the granted root, returning the resolved path
 * or throwing a `path_escape` CapabilityError for anything that resolves
 * outside it.
 */
export const clamp = Object.assign(clampExec, { path: clampPath });
