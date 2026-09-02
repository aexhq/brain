import type { AppToolRegistry } from "./app.js";
import { BrainError } from "./errors.js";
import type { SessionStreamEvent } from "./types.js";

/** What the pump needs from the client: the live stream and the result POST. Narrow on
 * purpose so tests can drive it without a BrainClient. */
export interface PumpTransport {
  stream(sessionId: string, after: number, signal?: AbortSignal): AsyncGenerator<SessionStreamEvent>;
  request<T>(method: string, path: string, body?: unknown, idempotencyKey?: string): Promise<T>;
}

interface ToolCallData {
  readonly deadline_ms?: number;
  readonly binding?: { readonly hosting?: string };
  readonly invocation?: { readonly call_id?: string; readonly name?: string; readonly input?: unknown };
  readonly target_sequence?: number;
}

/**
 * Serves a session's client-hosted tools off its event feed: watches `tool_call_started`
 * records whose binding says `client`, runs the registered handler, and POSTs the
 * outcome back under the record's sequence. `tool_cancel_started` records abort the
 * local handler; the stream reconnects from the last seen sequence until the session
 * ends or the handle stops the pump. Delivery is best effort by design — the session's
 * deadline is the backstop for anything this process fails to answer.
 */
export class ClientToolPump {
  private stopped = false;
  private readonly controller = new AbortController();
  /** sequence -> call_id for in-flight handlers, so a cancellation (which names the
   * call's started record) can abort the local call (which the registry names by call
   * id). */
  private readonly inFlight = new Map<number, string>();

  constructor(
    private readonly transport: PumpTransport,
    private readonly sessionId: string,
    private readonly registry: AppToolRegistry,
    private cursor: number,
  ) {}

  start(): void {
    void this.run();
  }

  /** The last journal sequence this pump has seen — where a successor resumes. */
  position(): number {
    return this.cursor;
  }

  stop(): void {
    this.stopped = true;
    this.controller.abort();
    for (const callId of this.inFlight.values()) this.registry.cancel(callId);
    this.inFlight.clear();
  }

  private async run(): Promise<void> {
    while (!this.stopped) {
      try {
        for await (const event of this.transport.stream(this.sessionId, this.cursor, this.controller.signal)) {
          if (event.sequence !== undefined) this.cursor = event.sequence;
          if (event.type === "session_ended") {
            this.stopped = true;
            break;
          }
          void this.handle(event);
        }
      } catch {
        // The stream dropped or lagged out; the reconnect below reads back what was
        // missed from the journal via the cursor.
      }
      if (this.stopped) return;
      await new Promise((resolve) => {
        const timer = setTimeout(resolve, 250);
        (timer as { unref?(): void }).unref?.();
      });
    }
  }

  private async handle(event: SessionStreamEvent): Promise<void> {
    const data = event.data as ToolCallData | undefined;
    if (data?.binding?.hosting !== "client") return;
    if (event.type === "tool_cancel_started") {
      const callId = typeof data.target_sequence === "number" ? this.inFlight.get(data.target_sequence) : undefined;
      if (callId !== undefined) this.registry.cancel(callId);
      return;
    }
    if (event.type !== "tool_call_started") return;
    const sequence = event.sequence;
    const invocation = data.invocation;
    if (sequence === undefined || typeof invocation?.call_id !== "string" || typeof invocation.name !== "string") return;
    // A client-hosted tool this pump has no handler for is someone else's to serve.
    if (!this.registry.has(invocation.name)) return;
    this.inFlight.set(sequence, invocation.call_id);
    const outcome = await this.registry.run({
      call_id: invocation.call_id,
      name: invocation.name,
      arguments: invocation.input,
      deadline_ms: typeof data.deadline_ms === "number" ? data.deadline_ms : 0,
    });
    this.inFlight.delete(sequence);
    if (this.stopped) return;
    try {
      await this.transport.request("POST", `/v1/sessions/${encodeURIComponent(this.sessionId)}/tool-results/${sequence}`, outcome, `tool-result-${sequence}`);
    } catch (error) {
      // A conflict means nobody is waiting any more — the call timed out or was
      // answered — and anything else is equally unactionable from here: the session's
      // deadline records the failure as a timeout the loop can read.
      if (!(error instanceof BrainError)) throw error;
    }
  }
}
