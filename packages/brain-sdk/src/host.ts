import type { Outcome, Program } from "./types.js";

/**
 * The SDK-owned host runtime for provisioned programs. An environment opts in per
 * program kind (`execute.esm()`, `execute.shell(...)`, `execute.http(...)`); this
 * module owns the dirty work — the ESM payload cache, provision-time validation,
 * script substitution, and mapping every result to the one Outcome envelope — so
 * environment authors never write it.
 */

/** A `tool/v1` manifest as `brain build` emits it — the only thing the host
 * (and Brain) reads about a tool. */
export interface ProvisionedToolManifest {
  readonly name: string;
  readonly description: string;
  readonly input_schema: Readonly<Record<string, unknown>>;
  readonly output_schema?: Readonly<Record<string, unknown>>;
  readonly needs: readonly string[];
  readonly binding_names: readonly string[];
  readonly program: Program;
}

/** The build artifact: manifest plus the program's payload — for `esm` the
 * self-contained single-file bundle whose sha-256 is the program identity; for
 * `shell` and `http` the same script or request template the manifest carries
 * inline. */
export interface ProvisionedToolArtifact {
  readonly manifest: ProvisionedToolManifest;
  readonly payload: string;
}

/** What a provisioned ESM bundle default-exports (`provisionedToolRuntime`
 * builds it). `parseInput` validates against the tool's own schema — the same
 * schema the manifest was generated from; `run` executes with the context the
 * host wires. */
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

/** Caches provisioned ESM payloads by content identity: each payload is imported
 * and validated once per process, so a broken bundle fails the provision
 * receipt at attach, never the first model call. */
export class EsmToolHost {
  private readonly artifacts = new Map<string, ProvisionedToolArtifact>();
  private readonly provisioned = new Map<string, Promise<ProvisionedToolModule>>();

  /** Register an artifact this process can serve. Only `esm` programs carry a
   * payload that travels out of band; artifacts of the other kinds are accepted
   * and ignored, so a build directory can be registered whole. */
  register(artifact: ProvisionedToolArtifact): void {
    const program = artifact?.manifest?.program;
    if (program === undefined || typeof artifact?.payload !== "string") throw new TypeError("a tool artifact needs a manifest with a program and its payload");
    if (program.kind !== "esm") return;
    if (typeof program.identity !== "string" || !/^[0-9a-f]{64}$/u.test(program.identity)) throw new TypeError("an ESM tool artifact needs its program identity");
    this.artifacts.set(program.identity, artifact);
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
    // The bundle is imported into the environment's own process and reaches its
    // resources through the platform's APIs directly. Payload identity is checked
    // above and the kernel admits artifacts; confinement is the platform's job
    // (the process, the VM, the browser), never a wrapper in this module.
    const loaded = (await import(`data:text/javascript;charset=utf-8,${encodeURIComponent(artifact.payload)}`)) as { readonly default?: unknown };
    const module = loaded.default as ProvisionedToolModule | undefined;
    if (module?.kind !== "brain.provisioned-tool/v1" || typeof module.parseInput !== "function" || typeof module.run !== "function") {
      throw new Error(`payload for ${artifact.manifest.name} is not a provisioned tool bundle`);
    }
    await module.initialize?.(context);
    return module;
  }
}

export interface HostedInvocation {
  readonly callId: string;
  readonly input: unknown;
  readonly deadlineMs: number;
  readonly signal: AbortSignal;
  readonly bindings: Readonly<Record<string, string>>;
}

/** Run `work` under the caller-owned deadline and cancellation and resolve to
 * exactly one Outcome: a thrown error keeps an identifier-shaped `code`, the
 * deadline maps to `timeout`, and a cancelled operation maps to `cancelled`. */
export async function invokeWithEnvelope(
  invocation: Pick<HostedInvocation, "deadlineMs" | "signal">,
  work: (call: { readonly signal: AbortSignal; readonly deadline: Date }) => unknown | Promise<unknown>,
): Promise<Outcome> {
  const controller = new AbortController();
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    controller.abort(new Error("tool deadline exceeded"));
  }, invocation.deadlineMs);
  const onCancel = () => controller.abort(invocation.signal.reason);
  invocation.signal.addEventListener("abort", onCancel, { once: true });
  if (invocation.signal.aborted) onCancel();
  const deadline = new Date(Date.now() + invocation.deadlineMs);
  try {
    const value = await Promise.race([Promise.resolve().then(() => work({ signal: controller.signal, deadline })), rejectOnAbort(controller.signal)]);
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

/** Run one hosted ESM invocation: input that fails the tool's schema is an
 * `invalid_input` error; everything else follows `invokeWithEnvelope`. */
export function invokeProvisioned(module: ProvisionedToolModule, invocation: HostedInvocation): Promise<Outcome> {
  let input: unknown;
  try {
    input = module.parseInput(invocation.input);
  } catch (error) {
    return Promise.resolve({ status: "error", error: { code: "invalid_input", message: messageOf(error) } });
  }
  return invokeWithEnvelope(invocation, ({ signal, deadline }) => module.run(input, Object.freeze({
    bindings: Object.freeze({ ...invocation.bindings }),
    signal,
    deadline,
    callId: invocation.callId,
    requestId: invocation.callId,
    progress: () => {},
  })));
}

/** Substitute a shell program's input references. `$name` and `${name}` are
 * replaced with the input property of that name, as text (strings verbatim,
 * anything else as JSON, null and undefined as nothing). A reference to a name
 * the input does not carry is left for the shell, so `$HOME` still means what
 * it always did. */
export function substituteScript(script: string, input: unknown): string {
  if (input === null || typeof input !== "object" || Array.isArray(input)) {
    throw Object.assign(new TypeError("shell tool input must be an object"), { code: "invalid_input" });
  }
  const values = input as Readonly<Record<string, unknown>>;
  return script.replace(/\$(?:\{([A-Za-z_][A-Za-z0-9_]*)\}|([A-Za-z_][A-Za-z0-9_]*))/gu, (reference: string, braced: string | undefined, bare: string | undefined) => {
    const name = braced ?? bare ?? "";
    if (!Object.hasOwn(values, name)) return reference;
    const value = values[name];
    if (value === undefined || value === null) return "";
    return typeof value === "string" ? value : JSON.stringify(value);
  });
}

/** Resolves never, rejects on abort — so a program that ignores its signal still
 * yields the invoke slot at the deadline (it keeps running in the background). */
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
export function messageOf(error: unknown): string {
  return String(error instanceof Error ? error.message : error).slice(0, 4096);
}
