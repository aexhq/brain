import { Brain, BrainError } from '@aexhq/brain';
import type { EventList } from '@aexhq/brain';

export function client(onUnauthorized: () => void) {
  return new Brain('http://brain.internal', '', async (input, init) => {
    const request = new Request(input, init);
    const url = new URL(request.url);
    const headers = new Headers(request.headers);
    headers.delete('authorization');
    const response = await fetch(`/api/brain${url.pathname}${url.search}`, {
      method: request.method,
      headers,
      credentials: 'same-origin',
      cache: 'no-store',
      redirect: 'error',
      signal: request.signal,
      body: ['GET', 'HEAD'].includes(request.method) ? undefined : await request.arrayBuffer(),
    });
    if (response.status === 401) onUnauthorized();
    return response;
  });
}

export async function recentEvents(id: string, signal: AbortSignal): Promise<EventList> {
  const response = await fetch(
    `/api/brain/sessions/${encodeURIComponent(id)}/events?tail=true&limit=200`,
    { signal, cache: 'no-store' },
  );
  const body = await response.json();
  if (!response.ok)
    throw new BrainError(
      response.status,
      body.error?.code,
      body.error?.message ?? 'Could not load activity',
    );
  return body;
}
