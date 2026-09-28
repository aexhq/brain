import type { HostToolRegistry } from "./host.js";
import type { HostActivity, HostSuspendRequest, HostSuspendResult, HostCommand, HostEvent, HostEventAck, HostResult, HostModelRequest, HostServiceRequest, ModelResult } from "./generated/session.js";
import type { SessionStreamEvent } from "./types.js";

export interface HostTransport {
  suspend(value: HostSuspendRequest): Promise<HostSuspendResult>;
  call(value: HostServiceRequest): Promise<unknown>;
  stream(signal?: AbortSignal, onOpen?: () => void): AsyncGenerator<SessionStreamEvent>;
  result(value: HostResult): Promise<HostEventAck>;
  emit(value: HostEvent): Promise<HostEventAck>;
  model(value: HostModelRequest): Promise<ModelResult>;
}

/** Answers the commands Brain sends this host: one registry per session placed here,
 * every in-flight call named by its session id and sequence. */
export class HostPump {
  private readonly controller = new AbortController();
  private readonly sessions = new Map<string, HostToolRegistry>();
  private readonly inFlight = new Map<string, number>();
  private readonly close: () => void;
  readonly closed: Promise<void>;
  private opening?: Promise<void>;
  private running?: Promise<void>;
  private connection?: AbortController;
  private activity?: HostActivity;
  private timer?: ReturnType<typeof setTimeout>;
  private suspension?: Promise<void>;
  private operations = 0;

  constructor(private readonly transport: HostTransport, private readonly idleTimeoutMs = 0) {
    let close!: () => void;
    this.closed = new Promise((resolve) => { close = resolve; });
    this.close = close;
  }

  register(sessionId: string, registry: HostToolRegistry): void {
    this.controller.signal.throwIfAborted();
    // Replayed creation must retain the registry that owns any in-flight calls.
    if (this.sessions.has(sessionId)) return;
    this.sessions.set(sessionId, registry);
  }

  unregister(sessionId: string): void {
    const registry = this.sessions.get(sessionId);
    if (registry !== undefined) {
      for (const [key, sequence] of this.inFlight) {
        if (key.startsWith(`${sessionId}:`)) registry.cancel(sequence);
      }
      this.sessions.delete(sessionId);
    }
  }

  async use<T>(operation: () => Promise<T>): Promise<T> {
    this.operations += 1;
    this.clearTimer();
    try {
      await this.start();
      return await operation();
    } finally {
      this.operations -= 1;
      this.scheduleIdle();
    }
  }

  async start(): Promise<void> {
    this.controller.signal.throwIfAborted();
    await this.suspension;
    if (this.connection?.signal.aborted) await this.running;
    this.controller.signal.throwIfAborted();
    if (this.opening !== undefined) return this.opening;
    const connection = new AbortController();
    this.connection = connection;
    let resolve!: () => void;
    let reject!: (error: unknown) => void;
    const ready = new Promise<void>((accept, refuse) => { resolve = accept; reject = refuse; });
    this.opening = ready;
    let opened = false;
    const running = this.run(AbortSignal.any([this.controller.signal, connection.signal]), () => {
      opened = true;
      resolve();
    });
    this.running = running.catch(error => { if (!opened) reject(error); }).finally(() => {
      if (!opened) reject(new Error("host command stream closed before opening"));
      this.activity = undefined;
      this.clearTimer();
      this.opening = undefined;
      this.running = undefined;
    });
    return ready;
  }

  stop(): void {
    this.controller.abort();
    this.clearTimer();
    this.cancelInFlight();
    this.sessions.clear();
    void Promise.allSettled([this.running, this.suspension]).then(this.close);
  }

  private clearTimer(): void {
    clearTimeout(this.timer);
    this.timer = undefined;
  }

  private scheduleIdle(): void {
    this.clearTimer();
    if (this.idleTimeoutMs === 0 || this.controller.signal.aborted || this.connection?.signal.aborted
      || !this.activity?.idle || this.operations !== 0 || this.inFlight.size !== 0 || this.suspension !== undefined) return;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      const connection = this.connection!;
      const pending = this.transport.suspend({ connection: this.activity!.connection }).then(result => {
        if (result.suspended) connection.abort();
      }, error => {
        connection.abort();
        throw error;
      });
      this.suspension = pending;
      void pending.finally(() => {
        if (this.suspension === pending) this.suspension = undefined;
        this.scheduleIdle();
      }).catch(() => {});
    }, this.idleTimeoutMs);
  }

  private async run(signal: AbortSignal, onOpen: () => void): Promise<void> {
    let opened = false;
    try {
      while (!signal.aborted) {
        try {
          for await (const event of this.transport.stream(signal, () => {
            opened = true;
            onOpen();
          })) {
            if (event.type === "activity") {
              this.activity = event.data as HostActivity;
              this.scheduleIdle();
            } else if (event.type === "command") {
              this.clearTimer();
              void this.handle(event.data as HostCommand).catch(() => {}).finally(() => this.scheduleIdle());
            }
          }
        } catch (error) {
          if (!opened || (error instanceof Error && "status" in error && [401, 403, 404].includes(error.status as number))) throw error;
        }
        // The server can close SSE before its suspension response arrives.
        await this.suspension?.catch(() => {});
        this.cancelInFlight("disconnect");
        this.activity = undefined;
        this.clearTimer();
        if (!opened) throw new Error("host command stream closed before opening");
        if (!signal.aborted) await new Promise((resolve) => setTimeout(resolve, 100));
      }
    } finally {
      this.cancelInFlight("disconnect");
    }
  }

  private cancelInFlight(reason: "cancel" | "disconnect" = "cancel"): void {
    for (const [key, sequence] of this.inFlight) {
      const separator = key.lastIndexOf(":");
      const registry = this.sessions.get(key.slice(0, separator));
      if (reason === "disconnect") registry?.disconnect(sequence);
      else registry?.cancel(sequence);
    }
    this.inFlight.clear();
  }

  private async handle(command: HostCommand): Promise<void> {
    const registry = this.sessions.get(command.session_id);
    if (registry === undefined) {
      if (command.operation.type === "invoke_tool") {
        await this.transport.result({
          session_id: command.session_id,
          sequence: command.sequence,
          update: { type: "finish",
          outcome: {
            status: "error",
            error: {
              code: "unknown_session",
              message: `session ${command.session_id} is not placed in this host`,
            },
          },
          },
        });
      }
      return;
    }
    if (command.operation.type === "cancel_tool") {
      const key = `${command.session_id}:${command.operation.target_sequence}`;
      const sequence = this.inFlight.get(key);
      if (sequence !== undefined) registry.cancel(sequence);
      return;
    }
    const key = `${command.session_id}:${command.sequence}`;
    this.inFlight.set(key, command.sequence);
    try {
      await registry.run({
      sessionId: command.session_id,
      environment: command.template ?? command.environment,
      sequence: command.sequence,
      name: command.operation.name,
      arguments: command.operation.input,
      ...(command.deadline_at_ms === undefined ? {} : { deadline_at_ms: command.deadline_at_ms }),
      model: request => this.transport.model({ session_id: command.session_id, sequence: command.sequence, request }),
      environments: request => this.transport.call({ session_id: command.session_id, sequence: command.sequence, call: { method: "environments", input: request } }),
      update: async (value) => (await this.transport.result({
        session_id: command.session_id,
        sequence: command.sequence,
        update: value,
      })).sequence,
      emit: async (kind, data) => (await this.transport.emit({
        session_id: command.session_id,
        sequence: command.sequence,
        event_type: kind,
        data,
      })).sequence,
    });
    } finally {
      this.inFlight.delete(key);
    }
  }
}
