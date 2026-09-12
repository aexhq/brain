import type { HostToolRegistry } from "./host.js";
import type { HostCommand, HostEvent, HostEventAck, HostResult } from "./generated/session.js";
import type { SessionStreamEvent } from "./types.js";

export interface HostTransport {
  stream(signal?: AbortSignal, onOpen?: () => void): AsyncGenerator<SessionStreamEvent>;
  result(value: HostResult): Promise<void>;
  emit(value: HostEvent): Promise<HostEventAck>;
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

  constructor(private readonly transport: HostTransport) {
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

  start(): Promise<void> {
    if (this.opening !== undefined) return this.opening;
    this.opening = new Promise((resolve, reject) => {
      let opened = false;
      void this.run(() => {
        opened = true;
        resolve();
      }).then(() => {
        if (!opened) reject(new Error("host command stream closed before opening"));
      }, (error: unknown) => {
        if (!opened) reject(error);
      });
    });
    return this.opening;
  }

  stop(): void {
    this.controller.abort();
    this.cancelInFlight();
    this.sessions.clear();
  }

  private async run(onOpen: () => void): Promise<void> {
    let opened = false;
    try {
      while (!this.controller.signal.aborted) {
        try {
          for await (const event of this.transport.stream(this.controller.signal, () => {
            opened = true;
            onOpen();
          })) {
            if (event.type === "command") void this.handle(event.data as HostCommand).catch(() => {});
          }
        } catch (error) {
          if (!opened) throw error;
        }
        this.cancelInFlight("disconnect");
        if (!opened) throw new Error("host command stream closed before opening");
        if (!this.controller.signal.aborted) await new Promise((resolve) => setTimeout(resolve, 100));
      }
    } finally {
      this.stop();
      this.close();
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
          outcome: {
            status: "error",
            error: {
              code: "unknown_session",
              message: `session ${command.session_id} is not placed in this host`,
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
    const outcome = await registry.run({
      sessionId: command.session_id,
      environment: command.environment,
      sequence: command.sequence,
      name: command.operation.name,
      arguments: command.operation.input,
      deadline_ms: Math.max(0, command.deadline_at_ms - Date.now()),
      emit: async (kind, data) => (await this.transport.emit({
        session_id: command.session_id,
        sequence: command.sequence,
        event_type: kind,
        data,
      })).sequence,
    });
    this.inFlight.delete(key);
    if (this.controller.signal.aborted) return;
    await this.transport.result({
      session_id: command.session_id,
      sequence: command.sequence,
      outcome,
    });
  }
}
