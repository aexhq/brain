import assert from "node:assert/strict";
import test from "node:test";

import { DurableEventBridge } from "../dist/index.js";

test("advances its cursor only after the queue acknowledges an event", async () => {
  let cursor = 0;
  let attempts = 0;
  const events = [{ event_id: "evt_1", sequence: 1, recorded_at_ms: 1, event_type: "turn_started", data: {} }];
  const brain = {
    async listSessions() { return { sessions: [{ session_id: "ses_1" }] }; },
    async readEvents(_sessionId, after) {
      return { events: after === 0 ? events : [], next_cursor: after === 0 ? 1 : after };
    },
  };
  const cursors = {
    async load() { return cursor; },
    async save(_sessionId, value) { cursor = value; },
  };
  const queue = {
    async publish() {
      attempts += 1;
      if (attempts === 1) throw new Error("queue unavailable");
    },
  };
  const bridge = new DurableEventBridge(brain, cursors, queue);
  await assert.rejects(bridge.runOnce(), /queue unavailable/u);
  assert.equal(cursor, 0);
  assert.equal(await bridge.runOnce(), 1);
  assert.equal(cursor, 1);
  assert.equal(attempts, 2);
});

test("drains more than one finite journal page in a run", async () => {
  let cursor = 0;
  const requested = [];
  const brain = {
    async listSessions() { return { sessions: [{ session_id: "ses_1" }] }; },
    async readEvents(_sessionId, after) {
      requested.push(after);
      if (after >= 2) return { events: [], next_cursor: after };
      const sequence = after + 1;
      return { events: [{ event_id: `evt_${sequence}`, sequence, recorded_at_ms: sequence, event_type: "test", data: {} }], next_cursor: sequence };
    },
  };
  const cursors = {
    async load() { return cursor; },
    async save(_sessionId, value) { cursor = value; },
  };
  const bridge = new DurableEventBridge(brain, cursors, { async publish() {} });
  assert.equal(await bridge.runOnce(), 2);
  assert.deepEqual(requested, [0, 1, 2]);
  assert.equal(cursor, 2);
});
