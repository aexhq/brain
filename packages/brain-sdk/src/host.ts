import { CapabilityError, type CapabilityHandles, type GrantSet } from "./capabilities.js";
import type { CapabilityName, Outcome } from "./types.js";

/**
 * The SDK-owned host runtime for provisioned ESM tools. An environment opts in
 * with one line (`host.esm()`); this module owns the dirty work — the payload
 * cache, provision-time validation, handle wiring, and mapping every result to
 * the one Outcome envelope — so environment authors never write it.
 */

/** A `tool/v1` manifest as `brain build` emits it — the only thing the host
 * (and Brain) reads about a tool. */
export interface ProvisionedToolManifest {
  readonly name: string;
  readonly description: string;
  readonly input_schema: Readonly<Record<string, unknown>>;
  readonly output_schema?: Readonly<Record<string, unknown>>;
  readonly requires: readonly CapabilityName[];
  readonly binding_names: readonly string[];
  readonly hosting?: "provisioned";
  readonly payload?: { readonly kind: "esm" | "component"; readonly identity: string };
}

/** The build artifact: manifest plus the self-contained single-file ESM bundle
 * whose sha-256 is the manifest's payload identity. */
export interface ProvisionedToolArtifact {
  readonly manifest: ProvisionedToolManifest;
  readonly payload: string;
}

/** What a provisioned ESM bundle default-exports (`provisionedToolRuntime`
 * builds it). `parseInput` validates against the tool's own schema — the same
 * schema the manifest was generated from; `run` executes with the typed
 * context the host wires. */
export interface ProvisionedToolModule {
  readonly kind: "brain.provisioned-tool/v1";
  initialize?(context: { readonly signal: AbortSignal; readonly requestId: string }): void | Promise<void>;
  parseInput(input: unknown): unknown;
  run(input: unknown, context: object): unknown | Promise<unknown>;
}

export async function payloadIdentity(payload: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(payload));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

/** Caches provisioned payloads by content identity: each payload is imported
 * and validated once per process, so a broken bundle fails the provision
 * receipt at attach, never the first model call. */
export class EsmToolHost {
  private readonly artifacts = new Map<string, ProvisionedToolArtifact>();
  private readonly provisioned = new Map<string, Promise<ProvisionedToolModule>>();

  register(artifact: ProvisionedToolArtifact): void {
    const identity = artifact?.manifest?.payload?.identity;
    if (typeof artifact?.payload !== "string" || typeof identity !== "string" || !/^[0-9a-f]{64}$/u.test(identity)) {
      throw new TypeError("an ESM tool artifact needs a payload and its manifest's payload identity");
    }
    if (artifact.manifest.payload?.kind !== "esm") throw new TypeError("only esm payloads can be hosted by host.esm");
    this.artifacts.set(identity, artifact);
  }

  /** Resolve one attach provision to its loaded module, importing and
   * initializing the bundle on first use. A rejection is forgotten so a later
   * attach may retry. */
  async provision(identity: string, context: { readonly signal: AbortSignal; readonly requestId: string }): Promise<ProvisionedToolModule> {
    const artifact = this.artifacts.get(identity);
    if (artifact === undefined) throw new Error(`no ESM payload is registered for identity ${identity}`);
    let pending = this.provisioned.get(identity);
    if (pending === undefined) {
      pending = this.load(artifact, identity, context);
      this.provisioned.set(identity, pending);
      pending.catch(() => this.provisioned.delete(identity));
    }
    return pending;
  }

  private async load(artifact: ProvisionedToolArtifact, identity: string, context: { readonly signal: AbortSignal; readonly requestId: string }): Promise<ProvisionedToolModule> {
    if ((await payloadIdentity(artifact.payload)) !== identity) throw new Error(`payload for ${artifact.manifest.name} does not match its declared identity`);
    // Trusted-artifact assumption, v1: the bundle is imported into the
    // environment server's own process. Payload identity is checked above and
    // the kernel admits artifacts, but there is no worker/vm confinement yet —
    // handles are direct in-process closures over the environment's providers,
    // and an RPC layer to carry them across a worker boundary is not worth its
    // weight until an untrusted-tool story needs it.
    const loaded = (await import(`data:text/javascript;charset=utf-8,${encodeURIComponent(artifact.payload)}`)) as { readonly default?: unknown };
    const module = loaded.default as ProvisionedToolModule | undefined;
    if (module?.kind !== "brain.provisioned-tool/v1" || typeof module.parseInput !== "function" || typeof module.run !== "function") {
      throw new Error(`payload for ${artifact.manifest.name} is not a provisioned tool bundle`);
    }
    await module.initialize?.(context);
    return module;
  }
}

/** Wrap providers into the handles a tool receives: same interface, with every
 * thrown failure surfaced as a typed CapabilityError. */
export function capabilityHandles(requires: readonly CapabilityName[], providers: Readonly<Partial<CapabilityHandles>>): Readonly<Partial<CapabilityHandles>> {
  const handles: Record<string, unknown> = {};
  for (const capability of requires) {
    const provider = providers[capability] as Readonly<Record<string, unknown>> | undefined;
    if (provider === undefined) throw new Error(`this environment does not provide the ${capability} capability`);
    const handle: Record<string, unknown> = {};
    for (const [name, member] of Object.entries(provider)) {
      if (typeof member !== "function") continue;
      handle[name] = async (...args: readonly unknown[]) => {
        try {
          return await (member as (...values: readonly unknown[]) => unknown).apply(provider, [...args]);
        } catch (error) {
          if (error instanceof CapabilityError) throw error;
          throw new CapabilityError(capability, "capability_error", String(error instanceof Error ? error.message : error).slice(0, 4096));
        }
      };
    }
    handles[capability] = Object.freeze(handle);
  }
  return Object.freeze(handles) as Readonly<Partial<CapabilityHandles>>;
}

export interface HostedInvocation {
  readonly callId: string;
  readonly input: unknown;
  readonly deadlineMs: number;
  readonly signal: AbortSignal;
  readonly handles: Readonly<Partial<CapabilityHandles>>;
  readonly bindings: Readonly<Record<string, string>>;
}

/** Run one hosted invocation and resolve to exactly one Outcome: input that
 * fails the tool's schema is an `invalid_input` error, a thrown error keeps an
 * identifier-shaped `code` (CapabilityErrors included), the caller-owned
 * deadline maps to `timeout`, and a cancelled operation maps to `cancelled`. */
export async function invokeProvisioned(module: ProvisionedToolModule, invocation: HostedInvocation): Promise<Outcome> {
  let input: unknown;
  try {
    input = module.parseInput(invocation.input);
  } catch (error) {
    return { status: "error", error: { code: "invalid_input", message: messageOf(error) } };
  }
  const controller = new AbortController();
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    controller.abort(new Error("tool deadline exceeded"));
  }, invocation.deadlineMs);
  const onCancel = () => controller.abort(invocation.signal.reason);
  invocation.signal.addEventListener("abort", onCancel, { once: true });
  if (invocation.signal.aborted) onCancel();
  const context = Object.freeze({
    ...invocation.handles,
    bindings: Object.freeze({ ...invocation.bindings }),
    signal: controller.signal,
    deadline: new Date(Date.now() + invocation.deadlineMs),
    callId: invocation.callId,
    requestId: invocation.callId,
    progress: () => {},
  });
  try {
    const value = await Promise.race([Promise.resolve(module.run(input, context)), rejectOnAbort(controller.signal)]);
    return { status: "ok", value: value ?? null };
  } catch (error) {
    if (timedOut) return { status: "timeout" };
    if (invocation.signal.aborted) return { status: "cancelled" };
    return { status: "error", error: { code: codeOf(error), message: messageOf(error) } };
  } finally {
    clearTimeout(timer);
    invocation.signal.removeEventListener("abort", onCancel);
  }
}

/** Resolves never, rejects on abort — so a tool that ignores its signal still
 * yields the invoke slot at the deadline (it keeps running in the background;
 * see the trusted-artifact note above). */
function rejectOnAbort(signal: AbortSignal): Promise<never> {
  return new Promise((_, reject) => {
    if (signal.aborted) reject(reasonOf(signal));
    else signal.addEventListener("abort", () => reject(reasonOf(signal)), { once: true });
  });
}
function reasonOf(signal: AbortSignal): unknown {
  return signal.reason instanceof Error ? signal.reason : new Error(String(signal.reason ?? "aborted"));
}
function codeOf(error: unknown): string {
  const code = (error as { readonly code?: unknown } | null)?.code;
  return typeof code === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(code) ? code : "tool_error";
}
function messageOf(error: unknown): string {
  return String(error instanceof Error ? error.message : error).slice(0, 4096);
}
