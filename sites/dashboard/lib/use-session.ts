'use client';

import { useEffect, useState } from 'react';
import { BrainError, type Brain, type Event, type Session } from '@aexhq/brain';
import { recentEvents } from './client';
import { appendEvent, data } from './events';

export function useSession(brain: Brain, id: string) {
  const [session, setSession] = useState<Session | null>(null);
  const [events, setEvents] = useState<Event[]>([]);
  const [liveText, setLiveText] = useState('');
  const [connection, setConnection] = useState('Connecting');
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    const signal = controller.signal;
    setSession(null);
    setEvents([]);
    setLiveText('');
    setError('');
    setConnection('Connecting');
    async function watch() {
      try {
        const [state, page] = await Promise.all([brain.get(id, signal), recentEvents(id, signal)]);
        if (signal.aborted) return;
        setSession(state);
        setEvents(page.events ?? []);
        let cursor = page.next_cursor;
        while (!signal.aborted) {
          try {
            setConnection('Live');
            for await (const event of brain.observe(id, { after: cursor, signal })) {
              if (signal.aborted) return;
              const payload = data(event);
              if (event.cursor) {
                cursor = event.cursor;
                setEvents((current) => appendEvent(current, event).slice(-500));
              }
              if (event.type === 'assistant.delta' && typeof payload.text === 'string')
                setLiveText((current) => current + payload.text);
              if (event.type === 'assistant.message' || event.type === 'session.status')
                setLiveText('');
              if (event.type === 'session.status' && typeof payload.status === 'string')
                setSession((current) =>
                  current
                    ? { ...current, status: String(payload.status), updated_at: event.time }
                    : current,
                );
            }
          } catch (error) {
            if (signal.aborted) return;
            if (error instanceof BrainError && [401, 403, 404, 409].includes(error.status))
              throw error;
          }
          if (signal.aborted) return;
          setConnection('Reconnecting');
          setLiveText('');
          await new Promise<void>((resolve) => {
            const done = () => {
              clearTimeout(timer);
              signal.removeEventListener('abort', done);
              resolve();
            };
            const timer = setTimeout(done, 2000);
            signal.addEventListener('abort', done, { once: true });
          });
        }
      } catch (error) {
        if (signal.aborted) return;
        setConnection('Disconnected');
        setError(
          error instanceof BrainError && error.code === 'cursor_unavailable'
            ? 'The saved activity cursor is no longer available. Reload the retained history to continue.'
            : (error as Error).message,
        );
      }
    }
    void watch();
    return () => controller.abort();
  }, [brain, id, revision]);
  return {
    session,
    events,
    liveText,
    connection,
    error,
    reload: () => setRevision((value) => value + 1),
  };
}
