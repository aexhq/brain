import { createHash, timingSafeEqual } from "node:crypto";
import type { IncomingMessage } from "node:http";
import type { Duplex } from "node:stream";

import { errorOutcome, MAX_DEADLINE_MS, normalizeOutcome, sign, signatureHeader, type InvokeFrame, type Outcome } from "./callback-wire.js";

/**
 * Where an environment sends callback-tool invocations. `channel` terminates the
 * app's outbound WebSocket (a browser page, a process behind NAT, connecting with
 * `appTools.connect`); `post` sends HMAC-signed POSTs to a backend app that mounted
 * `appTools(...).fetchHandler()`.
 */
export type CallbackRoute =
  | { readonly mode: "channel"; readonly token: string }
  | { readonly mode: "post"; readonly url: string; readonly signingKey: string };

export interface CallbackRouter {
  invoke(frame: InvokeFrame, signal: AbortSignal): Promise<Outcome>;
  /** Terminate an app's WebSocket upgrade on this router's channel. Returns false
   * when this router routes over POST and terminates no channel. */
  upgrade(request: IncomingMessage, socket: Duplex, head: Uint8Array): boolean;
  close(): void;
}

export function resolveCallbackRoute(value: unknown): CallbackRoute {
  if (value !== null && typeof value === "object") {
    const record = value as Record<string, unknown>;
    if (record.mode === "channel" && typeof record.token === "string" && record.token.length > 0) return { mode: "channel", token: record.token };
    if (record.mode === "post" && typeof record.url === "string" && /^https?:\/\//u.test(record.url) && typeof record.signingKey === "string" && record.signingKey.length > 0) {
      return { mode: "post", url: record.url, signingKey: record.signingKey };
    }
  }
  throw new TypeError("route.callbacks() needs { mode: \"channel\", token } or { mode: \"post\", url, signingKey }");
}

export function createCallbackRouter(route: CallbackRoute): CallbackRouter {
  return route.mode === "post" ? new PostRouter(route) : new ChannelRouter(route);
}

/** Refuse a socket that arrived as an HTTP upgrade with a plain HTTP response. */
export function refuseUpgrade(socket: Duplex, status: number, message: string): void {
  const reason = status === 401 ? "Unauthorized" : status === 404 ? "Not Found" : "Bad Request";
  try {
    socket.write(`HTTP/1.1 ${status} ${reason}\r\nConnection: close\r\nContent-Type: text/plain\r\nContent-Length: ${Buffer.byteLength(message)}\r\n\r\n${message}`);
  } catch {
    // The refusal is best-effort; the destroy below is what matters.
  }
  socket.destroy();
}

class PostRouter implements CallbackRouter {
  constructor(private readonly route: { readonly url: string; readonly signingKey: string }) {}

  async invoke(frame: InvokeFrame, signal: AbortSignal): Promise<Outcome> {
    const body = JSON.stringify(frame);
    const controller = new AbortController();
    let interruption: "timeout" | "cancelled" | undefined;
    const timer = setTimeout(() => { interruption = "timeout"; controller.abort(); }, frame.deadline_ms > MAX_DEADLINE_MS ? MAX_DEADLINE_MS : frame.deadline_ms);
    const onAbort = (): void => { interruption = "cancelled"; controller.abort(); };
    signal.addEventListener("abort", onAbort, { once: true });
    try {
      const response = await fetch(this.route.url, {
        method: "POST",
        headers: { "content-type": "application/json", [signatureHeader]: sign(body, this.route.signingKey) },
        body,
        signal: controller.signal,
      });
      const text = await response.text();
      if (!response.ok) return errorOutcome("app_unreachable", `the app answered ${response.status}`);
      let parsed: unknown;
      try {
        parsed = JSON.parse(text);
      } catch {
        return errorOutcome("invalid_outcome", "the app answered with a non-JSON body");
      }
      return normalizeOutcome(parsed);
    } catch (error) {
      if (interruption !== undefined) {
        this.cancelDownstream(frame.call_id);
        return { status: interruption };
      }
      return errorOutcome("app_unreachable", String(error instanceof Error ? error.message : error));
    } finally {
      clearTimeout(timer);
      signal.removeEventListener("abort", onAbort);
    }
  }

  private cancelDownstream(callId: string): void {
    const body = JSON.stringify({ cancel: callId });
    void fetch(this.route.url, {
      method: "POST",
      headers: { "content-type": "application/json", [signatureHeader]: sign(body, this.route.signingKey) },
      body,
      signal: AbortSignal.timeout(5_000),
    }).catch(() => {});
  }

  upgrade(): boolean { return false; }
  close(): void {}
}

class ChannelRouter implements CallbackRouter {
  private connection: ChannelConnection | undefined;
  private readonly pending = new Map<string, (outcome: Outcome) => void>();
  private closed = false;

  constructor(private readonly route: { readonly token: string }) {}

  invoke(frame: InvokeFrame, signal: AbortSignal): Promise<Outcome> {
    const connection = this.connection;
    if (connection === undefined) {
      return Promise.resolve(errorOutcome("app_disconnected", "no app is connected to this environment's callback channel"));
    }
    return new Promise((resolve) => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      const settle = (outcome: Outcome): void => {
        if (!this.pending.delete(frame.call_id)) return;
        if (timer !== undefined) clearTimeout(timer);
        signal.removeEventListener("abort", onAbort);
        resolve(outcome);
      };
      const onAbort = (): void => {
        connection.send({ cancel: frame.call_id });
        settle({ status: "cancelled" });
      };
      this.pending.set(frame.call_id, settle);
      timer = setTimeout(() => {
        connection.send({ cancel: frame.call_id });
        settle({ status: "timeout" });
      }, frame.deadline_ms > MAX_DEADLINE_MS ? MAX_DEADLINE_MS : frame.deadline_ms);
      signal.addEventListener("abort", onAbort, { once: true });
      connection.send(frame);
    });
  }

  upgrade(request: IncomingMessage, socket: Duplex, head: Uint8Array): boolean {
    if (this.closed) {
      refuseUpgrade(socket, 404, "this callback channel is closed");
      return true;
    }
    const key = request.headers["sec-websocket-key"];
    if ((request.headers.upgrade ?? "").toLowerCase() !== "websocket" || typeof key !== "string" || request.headers["sec-websocket-version"] !== "13") {
      refuseUpgrade(socket, 400, "expected a WebSocket upgrade");
      return true;
    }
    if (!this.authorized(request)) {
      refuseUpgrade(socket, 401, "invalid channel token");
      return true;
    }
    const accept = createHash("sha1").update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`).digest("base64");
    socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
    const connection: ChannelConnection = new ChannelConnection(socket, head, {
      message: (text) => this.deliver(text),
      close: () => {
        if (this.connection === connection) this.connection = undefined;
        // Calls in flight went down this socket; nothing will answer them now.
        for (const settle of [...this.pending.values()]) settle(errorOutcome("app_disconnected", "the app disconnected before answering"));
      },
    });
    // Last connection wins: a reconnecting app must displace its own half-dead socket.
    this.connection?.close();
    this.connection = connection;
    return true;
  }

  close(): void {
    this.closed = true;
    this.connection?.close();
    this.connection = undefined;
    for (const settle of [...this.pending.values()]) settle(errorOutcome("app_disconnected", "the callback channel is closed"));
  }

  private authorized(request: IncomingMessage): boolean {
    const url = new URL(request.url ?? "/", "http://environment");
    const bearer = /^Bearer (.+)$/u.exec(request.headers.authorization ?? "");
    const presented = url.searchParams.get("token") ?? bearer?.[1];
    if (typeof presented !== "string") return false;
    const expected = Buffer.from(this.route.token);
    const actual = Buffer.from(presented);
    return expected.length === actual.length && timingSafeEqual(expected, actual);
  }

  private deliver(text: string): void {
    let frame: unknown;
    try {
      frame = JSON.parse(text);
    } catch {
      return;
    }
    if (frame === null || typeof frame !== "object") return;
    const callId = (frame as Record<string, unknown>).call_id;
    if (typeof callId !== "string") return;
    this.pending.get(callId)?.(normalizeOutcome(frame));
  }
}

const maximumMessageBytes = 4 * 1024 * 1024;

/**
 * One terminated WebSocket connection: the minimal server half of RFC 6455 — the
 * handshake happened in the router; this owns text frames, ping/pong, close, and
 * continuation, with a hard message size bound. Small enough on purpose: the channel
 * carries only this contract's JSON frames, so a dependency earns nothing.
 */
class ChannelConnection {
  private buffer: Buffer = Buffer.alloc(0);
  private fragments: Buffer[] = [];
  private destroyed = false;

  constructor(private readonly socket: Duplex, head: Uint8Array, private readonly events: { message(text: string): void; close(): void }) {
    socket.on("data", (chunk: Buffer) => this.feed(chunk));
    socket.on("error", () => socket.destroy());
    socket.on("close", () => {
      if (this.destroyed) return;
      this.destroyed = true;
      this.events.close();
    });
    if (head.byteLength > 0) this.feed(Buffer.from(head.buffer, head.byteOffset, head.byteLength));
  }

  send(value: unknown): void {
    if (this.destroyed) return;
    this.socket.write(encodeFrame(0x1, Buffer.from(JSON.stringify(value))));
  }

  close(): void {
    if (this.destroyed) return;
    try {
      this.socket.write(encodeFrame(0x8, Buffer.alloc(0)));
    } catch {
      // Closing a broken socket is fine; destroy below is authoritative.
    }
    this.socket.destroy();
  }

  private feed(chunk: Buffer): void {
    if (this.destroyed) return;
    this.buffer = this.buffer.byteLength === 0 ? chunk : Buffer.concat([this.buffer, chunk]);
    if (this.buffer.byteLength > maximumMessageBytes + 14) {
      this.close();
      return;
    }
    for (;;) {
      const frame = decodeFrame(this.buffer);
      if (frame === undefined) return;
      if (frame === "invalid") {
        this.close();
        return;
      }
      this.buffer = this.buffer.subarray(frame.consumed);
      switch (frame.opcode) {
        case 0x0:
        case 0x1:
        case 0x2: {
          if (frame.opcode !== 0x0) this.fragments = [];
          this.fragments.push(frame.payload);
          if (this.fragments.reduce((total, part) => total + part.byteLength, 0) > maximumMessageBytes) {
            this.close();
            return;
          }
          if (frame.fin) {
            const text = Buffer.concat(this.fragments).toString("utf8");
            this.fragments = [];
            this.events.message(text);
          }
          break;
        }
        case 0x8:
          this.close();
          return;
        case 0x9:
          if (!this.destroyed) this.socket.write(encodeFrame(0xa, frame.payload));
          break;
        case 0xa:
          break;
        default:
          this.close();
          return;
      }
    }
  }
}

function encodeFrame(opcode: number, payload: Buffer): Buffer {
  let header: Buffer;
  if (payload.byteLength < 126) {
    header = Buffer.from([0x80 | opcode, payload.byteLength]);
  } else if (payload.byteLength < 65_536) {
    header = Buffer.alloc(4);
    header[0] = 0x80 | opcode;
    header[1] = 126;
    header.writeUInt16BE(payload.byteLength, 2);
  } else {
    header = Buffer.alloc(10);
    header[0] = 0x80 | opcode;
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(payload.byteLength), 2);
  }
  return Buffer.concat([header, payload]);
}

function decodeFrame(buffer: Buffer): { readonly fin: boolean; readonly opcode: number; readonly payload: Buffer; readonly consumed: number } | "invalid" | undefined {
  if (buffer.byteLength < 2) return undefined;
  const fin = (buffer[0]! & 0x80) !== 0;
  const opcode = buffer[0]! & 0x0f;
  const masked = (buffer[1]! & 0x80) !== 0;
  let length = buffer[1]! & 0x7f;
  let offset = 2;
  if (length === 126) {
    if (buffer.byteLength < 4) return undefined;
    length = buffer.readUInt16BE(2);
    offset = 4;
  } else if (length === 127) {
    if (buffer.byteLength < 10) return undefined;
    const wide = buffer.readBigUInt64BE(2);
    if (wide > BigInt(maximumMessageBytes)) return "invalid";
    length = Number(wide);
    offset = 10;
  }
  if (length > maximumMessageBytes) return "invalid";
  const mask = masked ? buffer.subarray(offset, offset + 4) : undefined;
  if (masked) offset += 4;
  if (buffer.byteLength < offset + length) return undefined;
  const payload = Buffer.from(buffer.subarray(offset, offset + length));
  if (mask !== undefined) {
    if (mask.byteLength < 4) return undefined;
    for (let index = 0; index < payload.byteLength; index += 1) payload[index] = payload[index]! ^ mask[index & 3]!;
  }
  return { fin, opcode, payload, consumed: offset + length };
}
