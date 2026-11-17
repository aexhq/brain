import assert from 'node:assert/strict';
import test from 'node:test';
import { appendEvent, modelUsage } from '../lib/events.ts';

const event = (id, request_id, attempt_id, input_tokens, output_tokens) => ({
  id,
  type: 'model.request',
  time: '2026-11-15T12:00:00Z',
  data: { request_id, attempt_id, usage: { input_tokens, output_tokens } },
});

test('usage replaces cumulative snapshots and uses physical attempts instead of logical totals', () => {
  const events = [
    event('1', 'request', undefined, 100, 50),
    event('2', 'request', 'a', 70, 20),
    event('3', 'request', 'a', 80, 30),
    event('4', 'request', 'b', 40, 10),
  ];
  assert.deepEqual(modelUsage(events), { input: 120, output: 40, partial: false, requests: 2 });
});
test('unknown usage stays unknown, and a reported zero remains zero', () => {
  assert.deepEqual(modelUsage([]), { input: null, output: null, partial: false, requests: 0 });
  assert.deepEqual(modelUsage([event('1', 'a')]), {
    input: null,
    output: null,
    partial: true,
    requests: 1,
  });
  assert.deepEqual(modelUsage([event('1', 'a', undefined, 0, 0)]), {
    input: 0,
    output: 0,
    partial: false,
    requests: 1,
  });
  assert.deepEqual(modelUsage([event('1', 'a', undefined, 20, 10), event('2', 'b')]), {
    input: 20,
    output: 10,
    partial: true,
    requests: 2,
  });
  const partial = event('1', 'a', undefined, 20, 10);
  partial.data.usage.availability = 'partial';
  assert.deepEqual(modelUsage([partial]), { input: 20, output: 10, partial: true, requests: 1 });
});
test('replayed event IDs do not duplicate activity', () => {
  const first = event('same', 'a');
  assert.deepEqual(appendEvent([first], first), [first]);
  assert.equal(appendEvent([first], event('new', 'b')).length, 2);
});
