import { createHash, randomUUID } from "node:crypto";

import * as z from "zod";

import {
  MAX_CUSTOMER_OBSERVATION_BYTES,
  MAX_CUSTOMER_REGISTRATIONS,
  MAX_CUSTOMER_REGISTRATION_DESCRIPTOR_BYTES,
  MAX_CUSTOMER_WS_FRAME_BYTES,
  MAX_TOOL_TERMINAL_INLINE_BYTES,
} from "./limits.js";
import { canonicalJson, type ClientRegistration, type ToolContext } from "./tools.js";

/** Leaves headroom below API Gateway WebSocket's 32 KiB frame limit. */
export { MAX_CUSTOMER_OBSERVATION_BYTES, MAX_CUSTOMER_WS_FRAME_BYTES };

export interface WebSocketRequest {
  readonly url: string;
  /** Short-lived scoped grant carried as a WebSocket subprotocol, never in the URL. */
  readonly protocol: string;
}

export type WebSocketFactory = (request: WebSocketRequest) => WebSocket;

export type CustomerObservation =
  | {
      type: "receipt";
      epoch: number;
      operation_id: string;
      request_digest: string;
      replayed: boolean;
    }
  | {
      type: "terminal";
      epoch: number;
      operation_id: string;
      request_digest: string;
      ok: boolean;
      output?: unknown;
      error?: string;
    };

/** One freshly minted socket grant plus authenticated HTTPS observation ingress. */
export interface CustomerEnvironmentChannel {
  readonly request: WebSocketRequest;
  observe(observation: CustomerObservation): Promise<void>;
}

export type CustomerEnvironmentConnector = () => Promise<CustomerEnvironmentChannel>;

interface OfferFrame {
  type: "offer";
  epoch: number;
  operation_id: string;
  request_digest: string;
  session_id: string;
  registration: string;
  name: string;
  contract_digest: string;
  input: unknown;
  deadline_at_ms: number;
}

interface CancelFrame {
  type: "cancel";
  epoch: number;
  operation_id: string;
  reason: string;
}

interface ReadyFrame {
  type: "ready";
  epoch: number;
}

interface RegisteredFrame {
  type: "registered";
  epoch: number;
  batch_id: string;
}

interface ErrorFrame {
  type: "error";
  code: string;
  message: string;
  batch_id?: string;
}

interface AckFrame {
  type: "ack";
  epoch: number;
  operation_id: string;
  request_digest: string;
  terminal_digest: string;
}

interface HeartbeatFrame {
  type: "heartbeat";
  epoch: number;
  nonce: string;
}

type ServerFrame = OfferFrame | CancelFrame | ReadyFrame | RegisteredFrame | ErrorFrame | AckFrame | HeartbeatFrame;

interface RetainedTerminal {
  readonly requestDigest: string;
  readonly terminalDigest: string;
  readonly observation: Extract<CustomerObservation, { type: "terminal" }>;
  readonly retainedAt: number;
}

interface RunningOperation {
  readonly controller: AbortController;
  readonly requestDigest: string;
}

interface RetainedUnknown {
  readonly requestDigest: string;
  readonly retainedAt: number;
}

interface PendingBatch {
  resolve(): void;
  reject(error: Error): void;
}

export interface CustomerEnvironmentOptions {
  readonly clientId: string;
  readonly processId?: string;
  readonly reconnectDelayMs?: number;
  readonly maxReconnectDelayMs?: number;
  /** Keeps API Gateway connections active. Defaults to four minutes; set to 0 to disable. */
  readonly heartbeatIntervalMs?: number;
  /** Exact heartbeat echo deadline. Defaults to 30 seconds. */
  readonly heartbeatTimeoutMs?: number;
  /** Align with the server's receipt-dedupe horizon. Defaults to 15 minutes. */
  readonly retentionTtlMs?: number;
  /** Shared bound across terminal replay entries and unknown-outcome tombstones. */
  readonly maxRetainedOperations?: number;
  /** Hard lifetime bound for immutable registrations in this process runner. */
  readonly maxRegistrations?: number;
  /** Hard aggregate encoded descriptor bound for immutable registrations. */
  readonly maxRegistrationDescriptorBytes?: number;
}

/**
 * Runs customer-app Tools in the application process that owns their closures.
 *
 * One client-ID-scoped connection multiplexes every bound session. A routine socket loss fences
 * new offers but does not cancel callbacks already assigned to this process: their operation-
 * scoped HTTPS observation channel remains valid independently of the socket. The same live
 * runner reconnects with its stable process ID, takes a new epoch, and re-registers all immutable
 * function contracts. Receipts/results use authenticated HTTPS, not WebSocket frames.
 */
export class CustomerEnvironment {
  readonly #connector: CustomerEnvironmentConnector;
  readonly #factory: WebSocketFactory;
  readonly #clientId: string;
  readonly #processId: string;
  readonly #reconnectDelayMs: number;
  readonly #maxReconnectDelayMs: number;
  readonly #heartbeatIntervalMs: number;
  readonly #heartbeatTimeoutMs: number;
  readonly #retentionTtlMs: number;
  readonly #maxRetainedOperations: number;
  readonly #maxRegistrations: number;
  readonly #maxRegistrationDescriptorBytes: number;
  readonly #registrations = new Map<string, ClientRegistration>();
  readonly #controllers = new Map<string, RunningOperation>();
  readonly #terminal = new Map<string, RetainedTerminal>();
  readonly #unknown = new Map<string, RetainedUnknown>();
  readonly #pendingBatches = new Map<string, PendingBatch>();
  #registrationBytes = 0;
  #socket: WebSocket | undefined;
  #channel: CustomerEnvironmentChannel | undefined;
  #epoch: number | undefined;
  #frameProof: string | undefined;
  #closed = false;
  #reconnectAttempt = 0;
  #opening: Promise<void> | undefined;
  #heartbeatTimer: ReturnType<typeof setTimeout> | undefined;
  #heartbeatDeadlineTimer: ReturnType<typeof setTimeout> | undefined;
  #pendingHeartbeatNonce: string | undefined;
  #reconnectSleepTimer: ReturnType<typeof setTimeout> | undefined;
  #wakeReconnectSleep: (() => void) | undefined;
  readonly #closedPromise: Promise<void>;
  #resolveClosed!: () => void;
  readonly ready: Promise<void>;

  constructor(
    connector: CustomerEnvironmentConnector,
    registrations: readonly ClientRegistration[],
    factory: WebSocketFactory,
    options: CustomerEnvironmentOptions,
  ) {
    this.#connector = connector;
    this.#factory = factory;
    this.#clientId = options.clientId;
    this.#processId = options.processId ?? `process:${randomUUID()}`;
    this.#reconnectDelayMs = options.reconnectDelayMs ?? 250;
    this.#maxReconnectDelayMs = options.maxReconnectDelayMs ?? 30_000;
    this.#heartbeatIntervalMs = options.heartbeatIntervalMs ?? 4 * 60 * 1_000;
    this.#heartbeatTimeoutMs = options.heartbeatTimeoutMs ?? 30_000;
    this.#retentionTtlMs = options.retentionTtlMs ?? 15 * 60 * 1_000;
    this.#maxRetainedOperations = options.maxRetainedOperations ?? 512;
    this.#maxRegistrations = options.maxRegistrations ?? MAX_CUSTOMER_REGISTRATIONS;
    this.#maxRegistrationDescriptorBytes = options.maxRegistrationDescriptorBytes
      ?? MAX_CUSTOMER_REGISTRATION_DESCRIPTOR_BYTES;
    if (
      this.#retentionTtlMs <= 0 || this.#maxRetainedOperations <= 0
      || this.#maxRegistrations <= 0 || this.#maxRegistrationDescriptorBytes <= 0
    ) {
      throw new TypeError("Customer Environment retention bounds must be positive");
    }
    this.#addRegistrations(registrations);
    if (this.#reconnectDelayMs < 0 || this.#maxReconnectDelayMs < this.#reconnectDelayMs) {
      throw new TypeError("Customer Environment reconnect bounds are invalid");
    }
    if (this.#heartbeatIntervalMs < 0 || this.#heartbeatTimeoutMs <= 0) {
      throw new TypeError("Customer Environment heartbeat bounds are invalid");
    }
    this.#closedPromise = new Promise<void>((resolve) => { this.#resolveClosed = resolve; });
    this.ready = this.#connectUntilReady(true);
    // Keep a caller that intentionally closes before awaiting readiness from creating a process-
    // level unhandled rejection. The original promise remains rejecting for explicit awaiters.
    void this.ready.catch(() => undefined);
  }

  close(): void {
    this.#closed = true;
    this.#resolveClosed();
    for (const [operationId, running] of this.#controllers) {
      this.#retainUnknown(operationId, running.requestDigest);
      running.controller.abort(new Error("Customer Environment closed"));
    }
    this.#controllers.clear();
    this.#socket?.close();
    this.#socket = undefined;
    this.#channel = undefined;
    this.#frameProof = undefined;
    this.#clearHeartbeat();
    if (this.#reconnectSleepTimer !== undefined) clearTimeout(this.#reconnectSleepTimer);
    this.#reconnectSleepTimer = undefined;
    this.#wakeReconnectSleep?.();
    this.#wakeReconnectSleep = undefined;
  }

  /** Add immutable registrations and wait for the gateway to acknowledge them. */
  async register(registrations: readonly ClientRegistration[]): Promise<void> {
    const added = this.#addRegistrations(registrations);
    await this.#ensureOpen();
    if (added.length > 0) await this.#registerBatches(added);
  }

  #addRegistrations(registrations: readonly ClientRegistration[]): ClientRegistration[] {
    const added: ClientRegistration[] = [];
    const prospective = new Map<string, ClientRegistration>(this.#registrations);
    let addedBytes = 0;
    for (const registration of registrations) {
      const current = prospective.get(registration.registration);
      if (current !== undefined) {
        if (
          current.name !== registration.name ||
          current.contractDigest !== registration.contractDigest ||
          current.handler !== registration.handler
        ) {
          throw new TypeError(
            `Customer Environment registration ${registration.registration} conflicts with its existing contract or handler`,
          );
        }
        continue;
      }
      prospective.set(registration.registration, registration);
      added.push(registration);
      addedBytes += registrationDescriptorBytes(registration);
    }
    if (prospective.size > this.#maxRegistrations) {
      throw new TypeError(`Customer Environment exceeds its ${this.#maxRegistrations} registration limit`);
    }
    if (this.#registrationBytes + addedBytes > this.#maxRegistrationDescriptorBytes) {
      throw new TypeError(
        `Customer Environment registration descriptors exceed ${this.#maxRegistrationDescriptorBytes} bytes`,
      );
    }
    for (const registration of added) {
      this.#registrations.set(registration.registration, registration);
    }
    this.#registrationBytes += addedBytes;
    return added;
  }

  #ensureOpen(): Promise<void> {
    if (this.#closed) return Promise.reject(new Error("Customer Environment is closed"));
    if (this.#socket !== undefined && this.#epoch !== undefined) return Promise.resolve();
    if (this.#opening !== undefined) return this.#opening;
    this.#opening = this.#open().finally(() => { this.#opening = undefined; });
    return this.#opening;
  }

  async #connectUntilReady(keepProcessAlive = false): Promise<void> {
    while (!this.#closed) {
      try {
        await this.#ensureOpen();
        return;
      } catch {
        await this.#waitReconnect(this.#nextReconnectDelay(), keepProcessAlive);
      }
    }
    throw new Error("Customer Environment is closed");
  }

  #nextReconnectDelay(): number {
    const exponent = Math.min(this.#reconnectAttempt++, 16);
    const ceiling = Math.min(this.#maxReconnectDelayMs, this.#reconnectDelayMs * 2 ** exponent);
    return ceiling === 0 ? 0 : Math.floor(ceiling / 2 + Math.random() * (ceiling / 2));
  }

  async #open(): Promise<void> {
    const connected = await Promise.race([
      this.#connector().then((channel) => ({ channel })),
      this.#closedPromise.then(() => ({ channel: undefined })),
    ]);
    if (connected.channel === undefined || this.#closed) throw new Error("Customer Environment is closed");
    const channel = connected.channel;
    const socket = this.#factory(channel.request);
    const frameProof = deriveFrameProof(channel.request.protocol);
    this.#channel = channel;
    this.#frameProof = frameProof;
    this.#socket = socket;
    await new Promise<void>((resolve, reject) => {
      let registered = false;
      const fail = (error: unknown): void => {
        if (!registered) reject(error instanceof Error ? error : new Error(String(error)));
      };
      socket.addEventListener("open", () => {
        try {
          this.#sendWs(socket, {
            type: "register",
            client_id: this.#clientId,
            process_id: this.#processId,
            proof: frameProof,
          });
        } catch (error) {
          fail(error);
        }
      });
      socket.addEventListener("message", (event) => {
        const text = String(event.data);
        if (new TextEncoder().encode(text).byteLength > MAX_CUSTOMER_WS_FRAME_BYTES) {
          socket.close();
          fail(new Error("Customer Environment command exceeds 24 KiB"));
          return;
        }
        let frame: ServerFrame;
        try {
          frame = JSON.parse(text) as ServerFrame;
        } catch (error) {
          socket.close();
          fail(error);
          return;
        }
        if (frame.type === "ready") {
          this.#epoch = frame.epoch;
          this.#reconnectAttempt = 0;
          void this.#registerBatches([...this.#registrations.values()]).then(() => {
            registered = true;
            this.#scheduleHeartbeat();
            resolve();
          }, fail);
          return;
        }
        if (frame.type === "registered") {
          if (frame.epoch === this.#epoch) {
            this.#pendingBatches.get(frame.batch_id)?.resolve();
            this.#pendingBatches.delete(frame.batch_id);
          }
          return;
        }
        if (frame.type === "ack") {
          if (frame.epoch === this.#epoch) {
            const retained = this.#terminal.get(frame.operation_id);
            if (
              retained !== undefined
              && retained.requestDigest === frame.request_digest
              && retained.terminalDigest === frame.terminal_digest
            ) {
              this.#terminal.delete(frame.operation_id);
            }
          }
          return;
        }
        if (frame.type === "heartbeat") {
          if (
            frame.epoch !== this.#epoch
            || this.#pendingHeartbeatNonce === undefined
            || frame.nonce !== this.#pendingHeartbeatNonce
          ) {
            socket.close();
            fail(new Error("Customer Environment heartbeat echo is invalid"));
            return;
          }
          if (this.#heartbeatDeadlineTimer !== undefined) clearTimeout(this.#heartbeatDeadlineTimer);
          this.#heartbeatDeadlineTimer = undefined;
          this.#pendingHeartbeatNonce = undefined;
          this.#scheduleHeartbeat();
          return;
        }
        if (frame.type === "error") {
          const error = new Error(`Customer Environment refused (${frame.code}): ${frame.message}`);
          if (frame.batch_id !== undefined) {
            this.#pendingBatches.get(frame.batch_id)?.reject(error);
            this.#pendingBatches.delete(frame.batch_id);
          } else {
            if (registered) socket.close();
            else fail(error);
          }
          return;
        }
        if (this.#epoch === undefined || frame.epoch !== this.#epoch) return;
        if (frame.type === "cancel") {
          this.#controllers.get(frame.operation_id)?.controller.abort(new Error(frame.reason));
          return;
        }
        void this.#offer(frame).catch(() => undefined);
      });
      socket.addEventListener("close", () => {
        if (this.#socket !== socket) return;
        this.#socket = undefined;
        this.#channel = undefined;
        this.#frameProof = undefined;
        this.#epoch = undefined;
        this.#clearHeartbeat();
        this.#pruneRetained();
        for (const batch of this.#pendingBatches.values()) batch.reject(new Error("Customer Environment disconnected"));
        this.#pendingBatches.clear();
        fail(new Error("Customer Environment disconnected during registration"));
        if (!this.#closed && registered) {
          void (async () => {
            await this.#waitReconnect(this.#nextReconnectDelay());
            await this.#connectUntilReady();
          })().catch(() => undefined);
        }
      });
      socket.addEventListener("error", (error) => {
        if (registered) socket.close();
        else fail(error);
      });
    });
  }

  #scheduleHeartbeat(): void {
    if (this.#heartbeatIntervalMs === 0 || this.#closed) return;
    if (this.#heartbeatTimer !== undefined) clearTimeout(this.#heartbeatTimer);
    this.#heartbeatTimer = setTimeout(() => {
      this.#heartbeatTimer = undefined;
      const socket = this.#socket;
      const epoch = this.#epoch;
      if (socket === undefined || epoch === undefined) return;
      try {
        const proof = this.#frameProof;
        if (proof === undefined) throw new Error("Customer Environment frame proof is unavailable");
        const nonce = randomUUID();
        this.#pendingHeartbeatNonce = nonce;
        this.#sendWs(socket, { type: "heartbeat", epoch, nonce, proof });
        this.#heartbeatDeadlineTimer = setTimeout(() => {
          this.#heartbeatDeadlineTimer = undefined;
          if (this.#pendingHeartbeatNonce === nonce && this.#socket === socket) socket.close();
        }, this.#heartbeatTimeoutMs);
        unrefTimer(this.#heartbeatDeadlineTimer);
      } catch {
        socket.close();
      }
    }, this.#heartbeatIntervalMs);
    unrefTimer(this.#heartbeatTimer);
  }

  #clearHeartbeat(): void {
    if (this.#heartbeatTimer !== undefined) clearTimeout(this.#heartbeatTimer);
    if (this.#heartbeatDeadlineTimer !== undefined) clearTimeout(this.#heartbeatDeadlineTimer);
    this.#heartbeatTimer = undefined;
    this.#heartbeatDeadlineTimer = undefined;
    this.#pendingHeartbeatNonce = undefined;
  }

  async #waitReconnect(delayMs: number, keepProcessAlive = false): Promise<void> {
    if (this.#closed) return;
    await new Promise<void>((resolve) => {
      let done = false;
      const finish = (): void => {
        if (done) return;
        done = true;
        if (this.#reconnectSleepTimer !== undefined) clearTimeout(this.#reconnectSleepTimer);
        this.#reconnectSleepTimer = undefined;
        this.#wakeReconnectSleep = undefined;
        resolve();
      };
      this.#wakeReconnectSleep = finish;
      this.#reconnectSleepTimer = setTimeout(finish, delayMs);
      if (!keepProcessAlive) unrefTimer(this.#reconnectSleepTimer);
    });
  }

  async #registerBatches(registrations: readonly ClientRegistration[]): Promise<void> {
    const socket = this.#socket;
    const epoch = this.#epoch;
    const proof = this.#frameProof;
    if (socket === undefined || epoch === undefined || proof === undefined) throw new Error("Customer Environment is not connected");
    const descriptors = registrations.map((registration) => ({
      registration: registration.registration,
      name: registration.name,
      contract_digest: registration.contractDigest,
    }));
    const batches: typeof descriptors[] = [];
    let current: typeof descriptors = [];
    for (const descriptor of descriptors) {
      const candidate = [...current, descriptor];
      const probe = { type: "register_tools", epoch, batch_id: "batch:00000000-0000-0000-0000-000000000000", proof, registrations: candidate };
      if (new TextEncoder().encode(JSON.stringify(probe)).byteLength > MAX_CUSTOMER_WS_FRAME_BYTES) {
        if (current.length === 0) throw new TypeError(`Customer Tool registration ${descriptor.registration} exceeds the frame limit`);
        batches.push(current);
        current = [descriptor];
      } else current = candidate;
    }
    if (current.length > 0) batches.push(current);
    await Promise.all(batches.map(async (batch) => {
      const batchId = `batch:${randomUUID()}`;
      const acknowledged = new Promise<void>((resolve, reject) => {
        this.#pendingBatches.set(batchId, { resolve, reject });
      });
      this.#sendWs(socket, { type: "register_tools", epoch, batch_id: batchId, proof, registrations: batch });
      await acknowledged;
    }));
  }

  async #offer(frame: OfferFrame): Promise<void> {
    this.#pruneRetained();
    const unknown = this.#unknown.get(frame.operation_id);
    if (unknown !== undefined) {
      if (unknown.requestDigest !== frame.request_digest) {
        await this.#sendConflict(frame);
      } else {
        await this.#sendTerminal(frame, false, undefined, "execution_unknown");
      }
      return;
    }
    const retained = this.#terminal.get(frame.operation_id);
    if (retained !== undefined) {
      if (retained.requestDigest === frame.request_digest) {
        await this.#observe({ ...retained.observation, epoch: frame.epoch });
      }
      else await this.#sendConflict(frame);
      return;
    }
    const running = this.#controllers.get(frame.operation_id);
    if (running !== undefined) {
      if (running.requestDigest !== frame.request_digest) {
        await this.#sendConflict(frame);
        return;
      }
      await this.#observe({
        type: "receipt",
        epoch: frame.epoch,
        operation_id: frame.operation_id,
        request_digest: frame.request_digest,
        replayed: true,
      });
      return;
    }
    // Unacknowledged facts are never evicted to admit more work. Withhold a receipt so the
    // coordinator retains ownership and can retry after acknowledgements or TTL expiry.
    if (this.#terminal.size + this.#unknown.size + this.#controllers.size >= this.#maxRetainedOperations) return;
    const channel = this.#channel;
    if (channel === undefined) return;
    const registration = this.#registrations.get(frame.registration);
    if (
      registration === undefined ||
      registration.name !== frame.name ||
      registration.contractDigest !== frame.contract_digest
    ) {
      await this.#sendTerminal(frame, false, undefined, "Customer Tool registration does not match the sealed contract");
      return;
    }
    const parsed = registration.input.safeParse(frame.input);
    if (!parsed.success) {
      await this.#sendTerminal(frame, false, undefined, `Customer Tool input is invalid: ${z.prettifyError(parsed.error)}`);
      return;
    }
    const controller = new AbortController();
    const admitted: RunningOperation = { controller, requestDigest: frame.request_digest };
    this.#controllers.set(frame.operation_id, admitted);
    const deadlineDelay = Math.max(0, frame.deadline_at_ms - Date.now());
    const deadline = setTimeout(
      () => controller.abort(new Error("Customer Tool deadline exceeded")),
      Math.min(deadlineDelay, 0x7fffffff),
    );
    try {
      await this.#observe({
        type: "receipt",
        epoch: frame.epoch,
        operation_id: frame.operation_id,
        request_digest: frame.request_digest,
        replayed: false,
      });
    } catch {
      // The effect has not started. Remove admission so a later offer can safely retry it.
      if (this.#controllers.get(frame.operation_id) === admitted) {
        this.#controllers.delete(frame.operation_id);
      }
      clearTimeout(deadline);
      return;
    }
    const context: ToolContext = {
      signal: controller.signal,
      operationId: frame.operation_id,
      sessionId: frame.session_id,
      deadlineMs: frame.deadline_at_ms,
    };
    let terminal: { ok: true; output: unknown } | { ok: false; error: string };
    try {
      if (controller.signal.aborted || Date.now() >= frame.deadline_at_ms) {
        terminal = {
          ok: false,
          error: boundedUtf8(
            controller.signal.reason instanceof Error
              ? controller.signal.reason.message
              : "Customer Tool was cancelled before execution",
            4 * 1024,
          ),
        };
      } else {
        try {
          let output = await registration.handler(parsed.data, context);
          if (registration.output !== undefined) output = registration.output.parse(output);
          const encoded = JSON.stringify(output);
          if (encoded === undefined) throw new Error("Customer Tool result must be JSON");
          if (new TextEncoder().encode(encoded).byteLength > MAX_TOOL_TERMINAL_INLINE_BYTES) {
            throw new Error(
              `Customer Tool result exceeds ${MAX_TOOL_TERMINAL_INLINE_BYTES} inline bytes; persist large data and return a reference`,
            );
          }
          // Materialize the exact JSON fact once. A mutable alias or stateful toJSON must not alter a
          // retained terminal outcome across an ambiguous HTTPS response or later replay.
          terminal = { ok: true, output: JSON.parse(encoded) as unknown };
        } catch (error) {
          terminal = {
            ok: false,
            error: boundedUtf8(error instanceof Error ? error.message : String(error), 4 * 1024),
          };
        }
      }
    } finally {
      clearTimeout(deadline);
      if (this.#controllers.get(frame.operation_id) === admitted) {
        this.#controllers.delete(frame.operation_id);
      }
    }
    if (this.#unknown.has(frame.operation_id)) return;
    if (terminal.ok) await this.#sendTerminal(frame, true, terminal.output, undefined, channel);
    else await this.#sendTerminal(frame, false, undefined, terminal.error, channel);
  }

  async #sendTerminal(
    frame: OfferFrame,
    ok: boolean,
    output?: unknown,
    error?: string,
    channel: CustomerEnvironmentChannel | undefined = this.#channel,
  ): Promise<void> {
    const retained = this.#terminal.get(frame.operation_id);
    if (retained !== undefined) {
      if (retained.requestDigest !== frame.request_digest) {
        await this.#sendConflict(frame, channel);
      } else {
        await this.#observe({ ...retained.observation, epoch: frame.epoch }, channel);
      }
      return;
    }
    const observation: Extract<CustomerObservation, { type: "terminal" }> = {
      type: "terminal",
      epoch: frame.epoch,
      operation_id: frame.operation_id,
      request_digest: frame.request_digest,
      ok,
      ...(ok ? { output } : { error: error ?? "Customer Tool failed" }),
    };
    if (new TextEncoder().encode(JSON.stringify(observation)).byteLength > MAX_CUSTOMER_OBSERVATION_BYTES) {
      throw new TypeError("Customer Tool terminal observation exceeds the HTTPS ingress limit");
    }
    this.#terminal.delete(frame.operation_id);
    this.#unknown.delete(frame.operation_id);
    this.#terminal.set(frame.operation_id, {
      requestDigest: frame.request_digest,
      terminalDigest: customerTerminalDigest(observation),
      observation,
      retainedAt: Date.now(),
    });
    this.#pruneRetained();
    await this.#observe(observation, channel);
  }

  async #sendConflict(
    frame: OfferFrame,
    channel: CustomerEnvironmentChannel | undefined = this.#channel,
  ): Promise<void> {
    await this.#observe({
      type: "terminal",
      epoch: frame.epoch,
      operation_id: frame.operation_id,
      request_digest: frame.request_digest,
      ok: false,
      error: "operation_id was reused with a different request_digest",
    }, channel);
  }

  #retainUnknown(operationId: string, requestDigest: string): void {
    this.#unknown.delete(operationId);
    this.#unknown.set(operationId, { requestDigest, retainedAt: Date.now() });
    this.#pruneRetained();
  }

  #pruneRetained(): void {
    const cutoff = Date.now() - this.#retentionTtlMs;
    for (const [operationId, retained] of this.#terminal) {
      if (retained.retainedAt >= cutoff) break;
      this.#terminal.delete(operationId);
    }
    for (const [operationId, retained] of this.#unknown) {
      if (retained.retainedAt >= cutoff) break;
      this.#unknown.delete(operationId);
    }
  }

  async #observe(
    observation: CustomerObservation,
    channel: CustomerEnvironmentChannel | undefined = this.#channel,
  ): Promise<void> {
    if (channel === undefined) throw new Error("Customer Environment observation channel is unavailable");
    let last: unknown;
    for (let attempt = 0; attempt < 2; attempt += 1) {
      try {
        await channel.observe(observation);
        return;
      } catch (error) {
        last = error;
      }
    }
    throw last instanceof Error ? last : new Error(String(last));
  }

  #sendWs(socket: WebSocket, frame: unknown): void {
    const text = JSON.stringify(frame);
    if (new TextEncoder().encode(text).byteLength > MAX_CUSTOMER_WS_FRAME_BYTES) {
      throw new TypeError("Customer Environment WebSocket frame exceeds 24 KiB");
    }
    socket.send(text);
  }
}

/** Exact terminal fact digest used by Brain's post-commit acknowledgement. */
export function customerTerminalDigest(
  observation: Extract<CustomerObservation, { type: "terminal" }>,
): string {
  return createHash("sha256").update(canonicalJson({
    operation_id: observation.operation_id,
    request_digest: observation.request_digest,
    ok: observation.ok,
    output: observation.ok ? (observation.output ?? null) : null,
    error: observation.ok ? null : (observation.error ?? null),
  })).digest("hex");
}

function boundedUtf8(value: string, maxBytes: number): string {
  const bytes = new TextEncoder().encode(value);
  if (bytes.byteLength <= maxBytes) return value;
  return `${new TextDecoder().decode(bytes.slice(0, Math.max(0, maxBytes - 3)))}...`;
}

function unrefTimer(timer: ReturnType<typeof setTimeout>): void {
  if (typeof timer === "object" && timer !== null && "unref" in timer) {
    (timer as { unref(): void }).unref();
  }
}

function registrationDescriptorBytes(registration: ClientRegistration): number {
  return new TextEncoder().encode(JSON.stringify({
    registration: registration.registration,
    name: registration.name,
    contract_digest: registration.contractDigest,
  })).byteLength;
}

/** Domain-separated proof bound to one consumed connect grant. */
export function deriveFrameProof(protocol: string): string {
  return createHash("sha256")
    .update("aex.customer-environment.frame-proof\0", "utf8")
    .update(protocol, "utf8")
    .digest("hex");
}
