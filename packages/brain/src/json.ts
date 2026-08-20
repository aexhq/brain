import { OutputSchemaError, SessionError } from "./errors.js";

/** RFC 8785 canonical JSON. Object keys sort by UTF-16 code units, as required by JCS. */
export function canonicalize(value: unknown): string {
  const visit = (current: unknown): unknown => {
    if (Array.isArray(current)) return current.map(visit);
    if (current !== null && typeof current === "object") {
      const object = current as Record<string, unknown>;
      return Object.fromEntries(
        Object.keys(object)
          .filter((key) => object[key] !== undefined)
          .sort()
          .map((key) => [key, visit(object[key])]),
      );
    }
    if (typeof current === "number" && !Number.isFinite(current)) {
      throw new OutputSchemaError("The output schema contains a non-finite number");
    }
    return current;
  };

  const encoded = JSON.stringify(visit(value));
  if (encoded === undefined) {
    throw new OutputSchemaError("The output schema is not JSON-serialisable");
  }
  return encoded;
}

export async function jcsSha256(value: unknown): Promise<string> {
  const subtle = globalThis.crypto?.subtle;
  if (subtle === undefined) {
    throw new OutputSchemaError("This runtime does not provide Web Crypto SHA-256 support");
  }
  const bytes = new TextEncoder().encode(canonicalize(value));
  const digest = await subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function randomIdempotencyKey(): string {
  const crypto = globalThis.crypto;
  if (crypto === undefined) {
    throw new SessionError("This runtime does not provide secure random values");
  }
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x40;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0"));
  return `${hex.slice(0, 4).join("")}-${hex.slice(4, 6).join("")}-${hex.slice(6, 8).join("")}-${hex.slice(8, 10).join("")}-${hex.slice(10).join("")}`;
}
