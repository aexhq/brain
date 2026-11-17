import type { Event, ModelRequest } from '@aexhq/brain';

export function data(event: Event): Record<string, unknown> {
  return event.data && typeof event.data === 'object'
    ? (event.data as Record<string, unknown>)
    : {};
}

export function appendEvent(events: Event[], event: Event): Event[] {
  if (events.some((item) => item.id === event.id)) return events;
  return [...events, event];
}

export function modelUsage(events: Event[]) {
  const observations = new Map<string, ModelRequest>();
  for (const event of events) {
    if (event.type !== 'model.request') continue;
    const request = event.data as ModelRequest;
    observations.set(`${request.request_id}:${request.attempt_id ?? ''}`, request);
  }
  const attempted = new Set(
    [...observations.values()].filter((item) => item.attempt_id).map((item) => item.request_id),
  );
  const requests = [...observations.values()].filter(
    (item) => item.attempt_id || !attempted.has(item.request_id),
  );
  let input = 0,
    output = 0,
    missingInput = 0,
    missingOutput = 0;
  let partial = false;
  for (const request of requests) {
    if (request.usage?.availability === 'partial' || request.usage?.availability === 'unknown')
      partial = true;
    if (request.usage?.input_tokens == null) missingInput++;
    else input += request.usage.input_tokens;
    if (request.usage?.output_tokens == null) missingOutput++;
    else output += request.usage.output_tokens;
  }
  return {
    input: requests.length && missingInput < requests.length ? input : null,
    output: requests.length && missingOutput < requests.length ? output : null,
    partial: partial || missingInput > 0 || missingOutput > 0,
    requests: requests.length,
  };
}

export function text(value: unknown): string {
  if (typeof value === 'string') return value;
  return JSON.stringify(value, null, 2) ?? '';
}
