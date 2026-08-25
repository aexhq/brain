import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";

import { componentContracts, type ComponentKind } from "./generated/components.js";

export const MAX_COMPONENT_BYTES = 32 * 1024 * 1024;

export type ComponentAsset = URL | Uint8Array;
export type SealedComponentAsset =
  | Readonly<{ kind: "url"; href: string }>
  | Readonly<{ kind: "inline"; base64: string }>;
export type ToolContextGrant = "environment" | "journal" | "storage" | "children" | "parent";

export interface ComponentMetadata {
  readonly name?: string;
  readonly source?: string;
}

export interface ComponentExtension<Kind extends ComponentKind = ComponentKind, Config = unknown> {
  readonly kind: "brain.component";
  readonly extension: Kind;
  readonly world: (typeof componentContracts)[Kind]["world"];
  readonly contractDigest: (typeof componentContracts)[Kind]["digest"];
  readonly asset: SealedComponentAsset;
  readonly config: Config;
  readonly grants: readonly ToolContextGrant[];
  readonly metadata: Readonly<ComponentMetadata>;
}

export interface ComponentOptions {
  readonly grants?: readonly ToolContextGrant[];
  readonly metadata?: ComponentMetadata;
}

export interface WireComponent {
  kind: ComponentKind;
  world: string;
  contract_digest: string;
  component_digest: string;
  component_base64: string;
  bytes: number;
  config: unknown;
  grants: ToolContextGrant[];
  metadata: ComponentMetadata;
}

const GRANTS = new Set<ToolContextGrant>([
  "environment",
  "journal",
  "storage",
  "children",
  "parent",
]);

export function component<Kind extends ComponentKind, Config>(
  extension: Kind,
  asset: ComponentAsset,
  config: Config,
  options: ComponentOptions = {},
): ComponentExtension<Kind, Config> {
  const contract = componentContracts[extension];
  if (contract === undefined) throw new TypeError(`Unknown Brain component kind ${String(extension)}`);
  const sealedAsset = sealAsset(asset);
  const sealedConfig = cloneJson(config, "component config");
  const grants = normalizeGrants(extension, options.grants ?? []);
  const metadata = normalizeMetadata(options.metadata ?? {});
  return Object.freeze({
    kind: "brain.component" as const,
    extension,
    world: contract.world,
    contractDigest: contract.digest,
    asset: sealedAsset,
    config: sealedConfig,
    grants,
    metadata,
  });
}

export function defineComponent<Kind extends ComponentKind, Options, Config>(definition: {
  readonly kind: Kind;
  readonly asset: ComponentAsset;
  readonly configure: (options: Options) => Config;
  readonly grants?: readonly ToolContextGrant[];
  readonly metadata?: ComponentMetadata;
}): (options: Options) => ComponentExtension<Kind, Config> {
  if (typeof definition.configure !== "function") {
    throw new TypeError("defineComponent requires a configure function");
  }
  return (options: Options) => component(
    definition.kind,
    definition.asset,
    definition.configure(options),
    {
      ...(definition.grants === undefined ? {} : { grants: definition.grants }),
      ...(definition.metadata === undefined ? {} : { metadata: definition.metadata }),
    },
  );
}

export async function prepareComponent(value: ComponentExtension): Promise<WireComponent> {
  assertComponent(value);
  const bytes = await loadAsset(value.asset);
  if (bytes.byteLength === 0 || bytes.byteLength > MAX_COMPONENT_BYTES) {
    throw new TypeError(`Brain component bytes must be between 1 and ${MAX_COMPONENT_BYTES}`);
  }
  return {
    kind: value.extension,
    world: value.world,
    contract_digest: value.contractDigest,
    component_digest: createHash("sha256").update(bytes).digest("hex"),
    component_base64: Buffer.from(bytes).toString("base64"),
    bytes: bytes.byteLength,
    config: value.config,
    grants: [...value.grants],
    metadata: { ...value.metadata },
  };
}

function assertComponent(value: ComponentExtension): void {
  if (value === null || typeof value !== "object" || value.kind !== "brain.component") {
    throw new TypeError("Expected a Brain component value");
  }
  const contract = componentContracts[value.extension];
  if (contract === undefined || value.world !== contract.world || value.contractDigest !== contract.digest) {
    throw new TypeError("Brain component contract identity is invalid");
  }
  assertSealedAsset(value.asset);
  assertJson(value.config, "component config");
  normalizeGrants(value.extension, value.grants);
  normalizeMetadata(value.metadata);
}

function sealAsset(asset: ComponentAsset): SealedComponentAsset {
  if (asset instanceof URL) return Object.freeze({ kind: "url" as const, href: asset.href });
  if (asset instanceof Uint8Array) {
    if (asset.byteLength === 0 || asset.byteLength > MAX_COMPONENT_BYTES) {
      throw new TypeError(`Brain component bytes must be between 1 and ${MAX_COMPONENT_BYTES}`);
    }
    return Object.freeze({ kind: "inline" as const, base64: Buffer.from(asset).toString("base64") });
  }
  throw new TypeError("Brain component asset must be a URL or Uint8Array");
}

function assertSealedAsset(asset: SealedComponentAsset): void {
  if (asset.kind === "inline") {
    if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(asset.base64)) {
      throw new TypeError("Brain component inline asset is not base64");
    }
    return;
  }
  if (asset.kind !== "url" || typeof asset.href !== "string") {
    throw new TypeError("Brain component asset is invalid");
  }
  new URL(asset.href);
}

async function loadAsset(asset: SealedComponentAsset): Promise<Uint8Array> {
  if (asset.kind === "inline") {
    if (asset.base64.length > Math.ceil(MAX_COMPONENT_BYTES / 3) * 4) {
      throw new TypeError(`Brain component asset exceeds ${MAX_COMPONENT_BYTES} bytes`);
    }
    return new Uint8Array(Buffer.from(asset.base64, "base64"));
  }
  const url = new URL(asset.href);
  if (url.protocol === "file:") {
    const metadata = await stat(url);
    if (!metadata.isFile() || metadata.size === 0 || metadata.size > MAX_COMPONENT_BYTES) {
      throw new TypeError(`Brain component file must contain 1 through ${MAX_COMPONENT_BYTES} bytes`);
    }
    return new Uint8Array(await readFile(url));
  }
  if (url.protocol !== "https:" && url.protocol !== "http:" && url.protocol !== "data:") {
    throw new TypeError(`Unsupported Brain component asset protocol ${url.protocol}`);
  }
  const response = await fetch(url);
  if (!response.ok) throw new TypeError(`Brain component asset returned HTTP ${response.status}`);
  const declared = response.headers.get("content-length");
  if (declared !== null && Number(declared) > MAX_COMPONENT_BYTES) {
    throw new TypeError(`Brain component asset exceeds ${MAX_COMPONENT_BYTES} bytes`);
  }
  if (response.body === null) throw new TypeError("Brain component asset response has no body");
  const chunks: Uint8Array[] = [];
  let total = 0;
  const reader = response.body.getReader();
  for (;;) {
    const { done, value: chunk } = await reader.read();
    if (done) break;
    total += chunk.byteLength;
    if (total > MAX_COMPONENT_BYTES) {
      await reader.cancel();
      throw new TypeError(`Brain component asset exceeds ${MAX_COMPONENT_BYTES} bytes`);
    }
    chunks.push(chunk);
  }
  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

function normalizeGrants(
  extension: ComponentKind,
  values: readonly ToolContextGrant[],
): readonly ToolContextGrant[] {
  if (extension !== "tool" && values.length !== 0) {
    throw new TypeError(`${extension} components cannot request Tool context grants`);
  }
  const unique = new Set<ToolContextGrant>();
  for (const value of values) {
    if (!GRANTS.has(value)) throw new TypeError(`Unknown Tool context grant ${String(value)}`);
    if (unique.has(value)) throw new TypeError(`Tool context grant ${value} is repeated`);
    unique.add(value);
  }
  return Object.freeze([...unique].sort());
}

function normalizeMetadata(value: ComponentMetadata): Readonly<ComponentMetadata> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError("component metadata must be an object");
  }
  if (value.name !== undefined) {
    if (typeof value.name !== "string" || value.name.length === 0 || value.name.length > 128) {
      throw new TypeError("component metadata name must contain 1 through 128 characters");
    }
  }
  if (value.source !== undefined) {
    if (typeof value.source !== "string" || value.source.length === 0 || value.source.length > 512) {
      throw new TypeError("component metadata source must contain 1 through 512 characters");
    }
  }
  return Object.freeze({
    ...(value.name === undefined ? {} : { name: value.name }),
    ...(value.source === undefined ? {} : { source: value.source }),
  });
}

function assertJson(value: unknown, label: string): void {
  assertJsonValue(value, label, new WeakSet<object>());
}

function cloneJson<Value>(value: Value, label: string): Value {
  assertJson(value, label);
  return freezeJson(JSON.parse(JSON.stringify(value)) as Value);
}

function freezeJson<Value>(value: Value): Value {
  if (value !== null && typeof value === "object") {
    for (const child of Object.values(value as Record<string, unknown>)) freezeJson(child);
    Object.freeze(value);
  }
  return value;
}

function assertJsonValue(value: unknown, label: string, seen: WeakSet<object>): void {
  if (value === null || typeof value === "string" || typeof value === "boolean") return;
  if (typeof value === "number") {
    if (!Number.isFinite(value)) throw new TypeError(`${label} contains a non-finite number`);
    return;
  }
  if (typeof value !== "object") throw new TypeError(`${label} must be JSON serializable`);
  if (seen.has(value)) throw new TypeError(`${label} contains a cycle`);
  seen.add(value);
  if (Array.isArray(value)) {
    for (let index = 0; index < value.length; index += 1) {
      if (!(index in value)) throw new TypeError(`${label} contains a sparse array`);
      assertJsonValue(value[index], label, seen);
    }
  } else {
    const prototype = Object.getPrototypeOf(value) as unknown;
    if (prototype !== Object.prototype && prototype !== null) {
      throw new TypeError(`${label} contains a non-plain object`);
    }
    for (const child of Object.values(value as Record<string, unknown>)) {
      assertJsonValue(child, label, seen);
    }
  }
  seen.delete(value);
}
