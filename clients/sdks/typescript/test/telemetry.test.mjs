import assert from 'node:assert/strict';
import { test } from 'node:test';
import { Brain, configureTelemetry } from '../dist/index.js';

test('SDK propagates trace context without recording credentials, IDs or payloads', async () => {
  const spans = [], measurements = [];
  configureTelemetry({
    tracerProvider: { getTracer: () => ({ startSpan: (name, options) => {
      const span = { name, options, ended: false, spanContext: () => ({ traceId: '1'.repeat(32), spanId: '2'.repeat(16), traceFlags: 1 }), setStatus(status) { this.status = status; }, end() { this.ended = true; } };
      spans.push(span); return span;
    } }) },
    meterProvider: { getMeter: () => ({ createHistogram: (name, options) => ({ record: (value, attributes) => measurements.push({ name, options, value, attributes }) }) }) },
  });
  const brain = new Brain('http://private-host', 'secret-token', async request => {
    assert.equal(request.headers.get('traceparent'), `00-${'1'.repeat(32)}-${'2'.repeat(16)}-01`);
    return Response.json({ id: 'customer-private-session' });
  });
  await brain.get('customer-private-session');
  assert.equal(spans.length, 1);
  assert.equal(spans[0].ended, true);
  assert.equal(measurements.length, 1);
  assert.deepEqual(Object.keys(measurements[0].attributes).sort(), ['operation', 'status']);
  assert.doesNotMatch(JSON.stringify({ spans, measurements }), /private-host|secret-token|customer-private-session/);
});
