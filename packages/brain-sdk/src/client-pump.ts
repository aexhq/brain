import type { AppToolRegistry } from "./app.js";
import type { HostCommand, HostEvent, HostEventAck, HostResult } from "./generated/session.js";
import type { SessionStreamEvent } from "./types.js";

export interface HostTransport {
  stream(signal?: AbortSignal, onOpen?: () => void): AsyncGenerator<SessionStreamEvent>;
  result(value: HostResult): Promise<void>;
  emit(value: HostEvent): Promise<HostEventAck>;
}

export class ResidentHostPump {
  private readonly controller = new AbortController();
  private readonly sessions = new Map<string, AppToolRegistry>();
  private readonly inFlight = new Map<string, string>();
  private readonly close: () => void;
  readonly closed: Promise<void>;
  private opening?: Promise<void>;

  constructor(private readonly transport: HostTransport) {
    let close!: () => void;
    this.closed = new Promise((resolve) => { close = resolve; });
    this.close = close;
  }

  register(sessionId: string, registry: AppToolRegistry): void {
    // Replayed creation must retain the registry that owns any in-flight calls.
    if (this.sessions.has(sessionId)) return;
    this.sessions.set(sessionId, registry);
  }

  unregister(sessionId: string): boolean {
    const registry = this.sessions.get(sessionId);
    if (registry !== undefined) {
      for (const [key, callId] of this.inFlight) {
        if (key.startsWith(`${sessionId}:`)) registry.cancel(callId);
      }
      this.sessions.delete(sessionId);
    }
    if (this.sessions.size === 0) this.stop();
    return this.sessions.size === 0;
  }

  start(): Promise<void> {
    if (this.opening !== undefined) return this.opening;
    this.opening = new Promise((resolve, reject) => {
      let opened = false;
      void this.run(() => {
        opened = true;
        resolve();
      }).then(() => {
        if (!opened) reject(new Error("resident host command stream closed before opening"));
      }, (error: unknown) => {
        if (!opened) reject(error);
      });
    });
    return this.opening;
  }

  stop(): void {
    this.controller.abort();
    this.cancelInFlight();
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
        this.cancelInFlight();
        if (!opened) throw new Error("resident host command stream closed before opening");
        if (!this.controller.signal.aborted) await new Promise((resolve) => setTimeout(resolve, 100));
      }
    } finally {
      this.stop();
      this.close();
    }
  }

  private cancelInFlight(): void {
    for (const [key, callId] of this.inFlight) {
      const separator = key.lastIndexOf(":");
      this.sessions.get(key.slice(0, separator))?.cancel(callId);
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
              message: `resident session ${command.session_id} is not registered`,
            },
          },
        });
      }
      return;
    }
    if (command.operation.type === "cancel_tool") {
      const key = `${command.session_id}:${command.operation.target_sequence}`;
      const callId = this.inFlight.get(key);
      if (callId !== undefined) registry.cancel(callId);
      return;
    }
    const invocation = command.operation.invocation;
    const key = `${command.session_id}:${command.sequence}`;
    this.inFlight.set(key, invocation.call_id);
    const outcome = await registry.run({
      call_id: invocation.call_id,
      name: invocation.name,
      arguments: invocation.input,
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
