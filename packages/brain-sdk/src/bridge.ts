import type { BrainClient } from "./client.js";
import type { SessionEvent } from "./types.js";

export interface EventCursorStore {
  load(sessionId: string): Promise<number | undefined>;
  save(sessionId: string, throughSequence: number): Promise<void>;
}

export interface EventQueue {
  publish(event: SessionEvent & { sessionId: string }): Promise<void>;
}

export class DurableEventBridge {
  constructor(
    private readonly brain: Pick<BrainClient, "listSessions" | "readEvents">,
    private readonly cursors: EventCursorStore,
    private readonly queue: EventQueue,
    private readonly maxPagesPerRun = 100,
  ) {
    if (!Number.isSafeInteger(maxPagesPerRun) || maxPagesPerRun < 1) {
      throw new TypeError("maxPagesPerRun must be a positive safe integer");
    }
  }

  async runOnce(): Promise<number> {
    let delivered = 0;
    const sessions = await this.brain.listSessions();
    for (const session of sessions) {
      let cursor = await this.cursors.load(session.id) ?? 0;
      for (let pageNumber = 0; pageNumber < this.maxPagesPerRun; pageNumber += 1) {
        const requestedCursor = cursor;
        const page = await this.brain.readEvents(session.id, cursor);
        for (const event of page.events) {
          await this.queue.publish({ ...event, sessionId: session.id });
          await this.cursors.save(session.id, event.sequence);
          cursor = event.sequence;
          delivered += 1;
        }
        if (page.nextCursor <= requestedCursor || page.events.length === 0) break;
        cursor = page.nextCursor;
      }
    }
    return delivered;
  }
}
