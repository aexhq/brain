import { createHmac, timingSafeEqual } from "node:crypto";

import type { Outcome } from "./types.js";

/** The one envelope every callback invocation resolves to — the same Outcome Brain
 * journals for every tool call, regardless of hosting (defined in types.ts). */
export type { Outcome };

/** One invocation as it crosses the app boundary: MCP data shapes over plain HTTP,
 * no framing ceremony. `deadline_ms` is the remaining budget, not an epoch. */
export interface InvokeFrame {
  readonly call_id: string;
  readonly name: string;
  readonly arguments: unknown;
  readonly deadline_ms: number;
}

/** Best-effort cancellation travels down the same channel as the invocation. */
export interface CancelFrame { readonly cancel: string }

export const signatureHeader = "x-brain-signature";

/** Ceiling on a wire-provided call deadline: generous next to the kernel's default
 * tool deadline, small enough that a hostile frame cannot pin a timer for hours. */
export const MAX_DEADLINE_MS = 600_000;

export function sign(body: string, signingKey: string): string {
  return createHmac("sha256", signingKey).update(body, "utf8").digest("hex");
}

export function verifySignature(body: string, signature: string, signingKey: string): boolean {
  if (!/^[0-9a-f]{64}$/u.test(signature)) return false;
  return timingSafeEqual(Buffer.from(sign(body, signingKey), "hex"), Buffer.from(signature, "hex"));
}

export function errorOutcome(code: string, message: string): Outcome {
  return { status: "error", error: { code, message: message.slice(0, 4096) } };
}

/** Accept only a well-formed Outcome from the other side of the wire; anything else
 * becomes a typed error instead of leaking garbage into the receipt. */
export function normalizeOutcome(value: unknown): Outcome {
  if (value !== null && typeof value === "object" && !Array.isArray(value)) {
    const record = value as Record<string, unknown>;
    if (record.status === "ok" && "value" in record) return { status: "ok", value: record.value ?? null };
    if (record.status === "timeout") return { status: "timeout" };
    if (record.status === "cancelled") return { status: "cancelled" };
    if (record.status === "error" && record.error !== null && typeof record.error === "object" && !Array.isArray(record.error)) {
      const error = record.error as Record<string, unknown>;
      if (typeof error.code === "string" && typeof error.message === "string") {
        const code = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(error.code) ? error.code : "app_error";
        return { status: "error", error: { code, message: error.message.slice(0, 4096), ...("details" in error ? { details: error.details } : {}) } };
      }
    }
  }
  return errorOutcome("invalid_outcome", "the app returned a malformed outcome");
}

export function parseInvokeFrame(value: unknown): InvokeFrame | undefined {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return undefined;
  const record = value as Record<string, unknown>;
  if (typeof record.call_id !== "string" || record.call_id.length === 0) return undefined;
  if (typeof record.name !== "string" || record.name.length === 0) return undefined;
  if (typeof record.deadline_ms !== "number" || !Number.isInteger(record.deadline_ms) || record.deadline_ms < 1) return undefined;
  if (!("arguments" in record)) return undefined;
  // The deadline arms a timer, so a frame must not be able to demand an unbounded one.
  return { call_id: record.call_id, name: record.name, arguments: record.arguments, deadline_ms: Math.min(record.deadline_ms, MAX_DEADLINE_MS) };
}

export function parseCancelFrame(value: unknown): CancelFrame | undefined {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return undefined;
  const cancel = (value as Record<string, unknown>).cancel;
  return typeof cancel === "string" && cancel.length > 0 ? { cancel } : undefined;
}
