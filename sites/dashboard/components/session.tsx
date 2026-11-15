'use client';

import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import type { Event, ModelRequest } from '@aexhq/brain';
import {
  ArrowDown,
  ArrowLeft,
  ArrowUp,
  ChevronDown,
  Code2,
  Download,
  FileText,
  MessageSquare,
  MoreHorizontal,
  Paperclip,
  Play,
  RefreshCw,
  Square,
  Terminal,
  Trash2,
  Wrench,
  X,
} from 'lucide-react';
import { data, modelUsage, text } from '../lib/events';
import { useSession } from '../lib/use-session';
import { useConnection } from './shell';
import { CopyButton, Dialog, ErrorNotice, Loading, Status, timestamp } from './ui';

export function SessionWorkspace({ id }: { id: string }) {
  const { brain } = useConnection();
  const router = useRouter();
  const { session, events, liveText, connection, error, reload } = useSession(brain, id);
  const [tab, setTab] = useState('conversation');
  const [selected, setSelected] = useState<Event | null>(null);
  const [confirm, setConfirm] = useState<'close' | 'delete' | null>(null);
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState('');
  const [draft, setDraft] = useState('');
  const [attachments, setAttachments] = useState<{ reference: string; name: string }[]>([]);
  const [sending, setSending] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const pending = useRef<{
    type: 'user.message';
    text: string;
    key: string;
    attachments?: string[];
  } | null>(null);
  const [history, setHistory] = useState<Event[]>([]);
  const [historyCursor, setHistoryCursor] = useState('');
  const [historyMore, setHistoryMore] = useState(false);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [allUsage, setAllUsage] = useState<Event[] | null>(null);
  const [usageLoading, setUsageLoading] = useState(false);
  const [usageTime, setUsageTime] = useState('');
  const usageController = useRef<AbortController | null>(null);
  const end = useRef<HTMLDivElement>(null);
  const file = useRef<HTMLInputElement>(null);
  useEffect(() => () => usageController.current?.abort(), []);
  const usage = useMemo(() => modelUsage(allUsage ?? events), [allUsage, events]);
  const turns = useMemo(() => {
    const groups = new Map<string, Event[]>();
    for (const event of events) {
      const key = event.input_id ?? (event.type === 'user.message' ? event.id : 'session');
      const group = groups.get(key) ?? [];
      group.push(event);
      groups.set(key, group);
    }
    return [...groups].filter(([, group]) =>
      group.some((event) =>
        ['user.message', 'assistant.message', 'tool.call', 'tool.result', 'session.error'].includes(
          event.type,
        ),
      ),
    );
  }, [events]);

  async function action(operation: 'interrupt' | 'resume' | 'close' | 'delete') {
    setBusy(true);
    setActionError('');
    try {
      await brain[operation](id);
      setConfirm(null);
      if (operation === 'delete') router.push('/');
      else reload();
    } catch (error) {
      setActionError((error as Error).message);
    } finally {
      setBusy(false);
    }
  }
  async function send(event: FormEvent) {
    event.preventDefault();
    if ((!draft.trim() && !attachments.length) || sending) return;
    setSending(true);
    setActionError('');
    pending.current ??= {
      type: 'user.message',
      text: draft,
      key: crypto.randomUUID(),
      ...(attachments.length ? { attachments: attachments.map((item) => item.reference) } : {}),
    };
    try {
      await brain.send(id, pending.current);
      pending.current = null;
      setDraft('');
      setAttachments([]);
      setUncertain(false);
      setTimeout(() => end.current?.scrollIntoView({ behavior: 'smooth', block: 'end' }), 100);
    } catch (error) {
      setUncertain(true);
      setActionError(
        `${(error as Error).message}. Delivery is not confirmed. Retry checks the same message identity.`,
      );
    } finally {
      setSending(false);
    }
  }
  async function upload(files: FileList | null) {
    if (!files?.length) return;
    setSending(true);
    setActionError('');
    try {
      for (const attachment of Array.from(files)) {
        if (attachment.size > 16 * 1024 * 1024)
          throw new Error('Each attachment must be 16 MiB or smaller');
        const stored = await brain.uploadAttachment(id, attachment.name, attachment);
        setAttachments((current) => [
          ...current,
          { reference: stored.reference, name: stored.name },
        ]);
      }
    } catch (error) {
      setActionError((error as Error).message);
    } finally {
      setSending(false);
      if (file.current) file.current.value = '';
    }
  }
  async function loadHistory(after = '') {
    setHistoryLoading(true);
    setActionError('');
    try {
      const page = await brain.history(id, after, 100);
      setHistory(page.events ?? []);
      setHistoryCursor(page.next_cursor);
      setHistoryMore((page.events ?? []).length === 100);
    } catch (error) {
      setActionError((error as Error).message);
    } finally {
      setHistoryLoading(false);
    }
  }
  async function loadUsage() {
    setUsageLoading(true);
    setActionError('');
    const controller = new AbortController();
    usageController.current?.abort();
    usageController.current = controller;
    try {
      const requests = new Map<string, Event>();
      let cursor = '';
      for (;;) {
        const page = await brain.history(id, cursor, 1000, controller.signal);
        for (const event of page.events ?? [])
          if (event.type === 'model.request') {
            const request = event.data as ModelRequest;
            requests.set(`${request.request_id}:${request.attempt_id ?? ''}`, event);
          }
        cursor = page.next_cursor;
        if ((page.events ?? []).length < 1000) break;
      }
      setAllUsage([...requests.values()]);
      setUsageTime(new Date().toLocaleTimeString());
    } catch (error) {
      if (!controller.signal.aborted) setActionError((error as Error).message);
    } finally {
      if (!controller.signal.aborted) setUsageLoading(false);
    }
  }
  function exportEvents() {
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(events, null, 2)], { type: 'application/json' }),
    );
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = 'session-activity.json';
    anchor.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  return (
    <div className="session-page">
      <div className="session-top">
        <Link href="/" className="back-link">
          <ArrowLeft size={15} />
          Sessions
        </Link>
        <div className="session-heading">
          <div>
            <h1>{id}</h1>
            <div className="session-meta">
              {session && (
                <>
                  <Status value={session.status} />
                  <span>{session.config.model}</span>
                  <span className="runtime-pill">{session.harness}</span>
                </>
              )}
            </div>
          </div>
          <div className="session-actions">
            {session?.status === 'running' && (
              <button
                className="button secondary"
                disabled={busy}
                onClick={() => void action('interrupt')}
              >
                <Square size={14} />
                Interrupt
              </button>
            )}
            {session && ['interrupted', 'failed'].includes(session.status) && (
              <button
                className="button primary"
                disabled={busy}
                onClick={() => void action('resume')}
              >
                <Play size={15} />
                Resume
              </button>
            )}
            <details className="action-menu">
              <summary className="icon-button" aria-label="Session actions">
                <MoreHorizontal size={20} />
              </summary>
              <div>
                <button onClick={exportEvents}>
                  <Download size={15} />
                  Export loaded events
                </button>
                {session?.status !== 'closed' && (
                  <button
                    onClick={(event) => {
                      event.currentTarget.closest('details')?.removeAttribute('open');
                      setConfirm('close');
                    }}
                  >
                    <Square size={15} />
                    Close session
                  </button>
                )}
                <button
                  className="danger-text"
                  onClick={(event) => {
                    event.currentTarget.closest('details')?.removeAttribute('open');
                    setConfirm('delete');
                  }}
                >
                  <Trash2 size={15} />
                  Delete session
                </button>
              </div>
            </details>
          </div>
        </div>
      </div>
      <div className="session-tabs">
        <div role="tablist" aria-label="Session views">
          {['conversation', 'events', 'configuration', 'usage'].map((name) => (
            <button
              key={name}
              role="tab"
              aria-selected={tab === name}
              onClick={() => {
                setTab(name);
                if (name === 'events' && !history.length) void loadHistory();
              }}
            >
              {name[0].toUpperCase() + name.slice(1)}
            </button>
          ))}
        </div>
        <span className={`stream-state ${connection === 'Live' ? 'is-live' : ''}`} role="status">
          <span />
          {connection}
        </span>
      </div>
      <div className="session-notices">
        <ErrorNotice message={error || actionError} />
        {error && (
          <button className="button secondary small" onClick={reload}>
            <RefreshCw size={14} />
            Reload activity
          </button>
        )}
        {connection === 'Reconnecting' && (
          <p className="notice">Reconnecting to activity. The agent may still be running.</p>
        )}
      </div>
      {!session ? (
        !error && <Loading label="Opening session…" />
      ) : (
        <>
          {tab === 'conversation' && (
            <div className="workspace-grid">
              <div className="conversation-column">
                <div className="conversation-feed">
                  <div className="activity-label">
                    <span>RECENT ACTIVITY</span>
                    <span>
                      {events.length} loaded events ·{' '}
                      <button
                        className="text-button"
                        onClick={() => {
                          setTab('events');
                          void loadHistory();
                        }}
                      >
                        Browse full history
                      </button>
                    </span>
                  </div>
                  {!turns.length && (
                    <div className="empty-state conversation-empty">
                      <span className="empty-icon">
                        <MessageSquare size={27} />
                      </span>
                      <h2>Ready when you are</h2>
                      <p>
                        Send a task to start this session. You can follow each response and tool
                        call here.
                      </p>
                    </div>
                  )}
                  {turns.map(([key, group], index) => (
                    <Turn
                      key={key}
                      sessionId={id}
                      events={group}
                      index={index}
                      onInspect={setSelected}
                    />
                  ))}
                  {liveText && (
                    <div className="assistant-message">
                      <div className="message-label">
                        <span className="agent-avatar">B</span>Brain{' '}
                        <span className="muted">Writing…</span>
                      </div>
                      <div className="message-text live-text">{liveText}</div>
                    </div>
                  )}
                  <div ref={end} />
                </div>
                <div className="composer-wrap">
                  {!['idle', 'running'].includes(session.status) ? (
                    <div className="inactive-composer">
                      {session.status === 'closed'
                        ? 'This session is closed. Its history remains available.'
                        : 'Resume this session before sending another message.'}
                      {session.status === 'closed' && (
                        <Link href="/sessions/new">
                          Start a new session <ArrowRightIcon />
                        </Link>
                      )}
                    </div>
                  ) : (
                    <form className="composer" onSubmit={send}>
                      <label className="sr-only" htmlFor="message">
                        Message
                      </label>
                      <textarea
                        id="message"
                        value={draft}
                        disabled={sending || uncertain}
                        onChange={(event) => setDraft(event.target.value)}
                        rows={3}
                        placeholder={
                          session.status === 'running'
                            ? 'Queue a follow-up for the next turn…'
                            : 'Give your agent a task…'
                        }
                        onKeyDown={(event) => {
                          if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
                            event.preventDefault();
                            event.currentTarget.form?.requestSubmit();
                          }
                        }}
                      />
                      {attachments.length > 0 && (
                        <div className="attachment-list">
                          {attachments.map((item) => (
                            <span key={item.reference}>
                              <FileText size={13} />
                              {item.name}
                              <button
                                type="button"
                                disabled={sending || uncertain}
                                aria-label={`Remove ${item.name}`}
                                onClick={() =>
                                  setAttachments((current) =>
                                    current.filter((other) => other.reference !== item.reference),
                                  )
                                }
                              >
                                <X size={12} />
                              </button>
                            </span>
                          ))}
                        </div>
                      )}
                      <div className="composer-footer">
                        <div>
                          <input
                            ref={file}
                            type="file"
                            className="sr-only"
                            tabIndex={-1}
                            multiple
                            onChange={(event) => void upload(event.target.files)}
                          />
                          <button
                            type="button"
                            className="icon-button"
                            aria-label="Attach files"
                            disabled={sending || uncertain}
                            onClick={() => file.current?.click()}
                          >
                            <Paperclip size={17} />
                          </button>
                          <span className="muted">
                            {session.status === 'running'
                              ? 'Messages queue after the active turn'
                              : 'Ctrl / ⌘ + Enter to send'}
                          </span>
                        </div>
                        <div>
                          {uncertain && (
                            <button
                              className="text-button"
                              type="button"
                              onClick={() => {
                                pending.current = null;
                                setUncertain(false);
                                setDraft('');
                                setAttachments([]);
                                setActionError('');
                              }}
                            >
                              Discard draft
                            </button>
                          )}
                          <button
                            className="send-button"
                            aria-label={
                              uncertain
                                ? 'Retry same message'
                                : session.status === 'running'
                                  ? 'Queue message'
                                  : 'Send message'
                            }
                            disabled={sending || (!draft.trim() && !attachments.length)}
                          >
                            {uncertain ? <RefreshCw size={17} /> : <ArrowUp size={18} />}
                          </button>
                        </div>
                      </div>
                    </form>
                  )}
                  <p className="composer-note">
                    {uncertain
                      ? 'The original message may have been accepted. Retrying keeps its identity.'
                      : 'Session configuration stays fixed. Create a new session to change it.'}
                  </p>
                </div>
              </div>
              <aside className="session-inspector">
                <div className="inspector-title">
                  SESSION DETAILS
                  <CopyButton value={id} label="Copy ID" />
                </div>
                <dl className="details">
                  <div>
                    <dt>Status</dt>
                    <dd>
                      <Status value={session.status} />
                    </dd>
                  </div>
                  <div>
                    <dt>Runtime</dt>
                    <dd>{session.harness}</dd>
                  </div>
                  <div>
                    <dt>Model</dt>
                    <dd>{session.config.model}</dd>
                  </div>
                  <div>
                    <dt>Created</dt>
                    <dd>{timestamp(session.created_at)}</dd>
                  </div>
                  <div>
                    <dt>Last activity</dt>
                    <dd>{timestamp(session.updated_at)}</dd>
                  </div>
                </dl>
                <div className="inspector-section">
                  <h3>
                    Available tools{' '}
                    <span className="count">{(session.descriptors ?? []).length}</span>
                  </h3>
                  {(session.descriptors ?? []).length ? (
                    (session.descriptors ?? []).map((tool) => (
                      <div className="bound-tool" key={tool.name}>
                        <Wrench size={14} />
                        <span>{tool.name}</span>
                      </div>
                    ))
                  ) : (
                    <p className="muted">No tools configured.</p>
                  )}
                </div>
                <div className="inspector-section">
                  <h3>Instructions</h3>
                  <p className="instruction-preview">
                    {session.config.instructions || 'Default runtime instructions.'}
                  </p>
                </div>
                <button
                  className="text-button"
                  onClick={() => end.current?.scrollIntoView({ behavior: 'smooth' })}
                >
                  <ArrowDown size={14} />
                  Jump to latest
                </button>
              </aside>
            </div>
          )}
          {tab === 'events' && (
            <section className="tab-content">
              <div className="section-title">
                <div>
                  <h2>Event history</h2>
                  <p className="muted">
                    Retained events in server order. Select a row to inspect its JSON.
                  </p>
                </div>
                <button
                  className="button secondary small"
                  disabled={historyLoading}
                  onClick={() => void loadHistory()}
                >
                  <RefreshCw size={14} />
                  From start
                </button>
              </div>
              <div className="panel">
                {historyLoading ? (
                  <Loading label="Loading history…" />
                ) : (
                  <div className="event-list">
                    {history.length ? (
                      history.map((event) => (
                        <button
                          key={event.id}
                          className="event-row"
                          onClick={() => setSelected(event)}
                        >
                          <span className="event-type">
                            <Code2 size={14} />
                            {event.type}
                          </span>
                          <code>{event.id.slice(0, 12)}</code>
                          <time>{timestamp(event.time)}</time>
                          <ChevronDown size={14} />
                        </button>
                      ))
                    ) : (
                      <p className="empty-state">No retained events.</p>
                    )}
                  </div>
                )}
                <div className="table-footer">
                  <span>{history.length} events on this page</span>
                  {historyMore && (
                    <button
                      className="button secondary small"
                      disabled={historyLoading}
                      onClick={() => void loadHistory(historyCursor)}
                    >
                      Next page
                    </button>
                  )}
                </div>
              </div>
            </section>
          )}
          {tab === 'configuration' && (
            <section className="tab-content configuration-tab">
              <div className="panel">
                <div className="panel-heading">
                  <h2>Session configuration</h2>
                  <CopyButton
                    value={JSON.stringify(
                      { harness: session.harness, config: session.config },
                      null,
                      2,
                    )}
                  />
                </div>
                <pre className="json-view">
                  {JSON.stringify(
                    {
                      harness: session.harness,
                      config: session.config,
                      tools: session.descriptors,
                      credential_bindings: session.credential_bindings ?? [],
                    },
                    null,
                    2,
                  )}
                </pre>
              </div>
              <p className="muted">
                Credentials are omitted by Brain. Configuration is fixed when a session is created.
              </p>
            </section>
          )}
          {tab === 'usage' && (
            <section className="tab-content">
              <div className="section-title">
                <div>
                  <h2>Reported model usage</h2>
                  <p className="muted">
                    {allUsage
                      ? `Full-history snapshot at ${usageTime}. Refresh to include later activity.`
                      : 'Based on loaded activity. Read the full history for session-wide usage.'}
                  </p>
                </div>
                <button
                  className="button secondary"
                  disabled={usageLoading}
                  onClick={() => void loadUsage()}
                >
                  <RefreshCw size={15} className={usageLoading ? 'spin' : ''} />
                  {usageLoading
                    ? 'Reading history…'
                    : allUsage
                      ? 'Refresh usage'
                      : 'Read full session usage'}
                </button>
              </div>
              <div className="stats usage-stats">
                <div>
                  <span>Reported input tokens</span>
                  <strong>{usage.input?.toLocaleString() ?? 'Unknown'}</strong>
                </div>
                <div>
                  <span>Reported output tokens</span>
                  <strong>{usage.output?.toLocaleString() ?? 'Unknown'}</strong>
                </div>
                <div>
                  <span>Observed requests / attempts</span>
                  <strong>{usage.requests}</strong>
                </div>
              </div>
              <div className="notice">
                {usage.partial
                  ? 'Some token quantities are unknown. Reported counts may be incomplete. '
                  : ''}
                Usage reflects provider reports. Brain does not calculate model charges; check your
                provider for billing.
              </div>
            </section>
          )}
        </>
      )}
      {selected && (
        <Dialog title={selected.type} onClose={() => setSelected(null)}>
          <div className="event-dialog-meta">
            <time>{timestamp(selected.time)}</time>
            <CopyButton value={JSON.stringify(selected, null, 2)} label="Copy JSON" />
          </div>
          <pre className="json-view">{JSON.stringify(selected, null, 2)}</pre>
        </Dialog>
      )}
      {confirm && (
        <Dialog
          title={confirm === 'delete' ? 'Delete this session?' : 'Close this session?'}
          onClose={() => {
            if (!busy) setConfirm(null);
          }}
        >
          <p>
            {confirm === 'delete'
              ? 'This stops active work and removes the session’s online history, attachments, and private state after cleanup. This cannot be undone.'
              : 'This permanently closes the session and stops active or queued work. Its history remains readable.'}
          </p>
          <p className="confirm-id">{id}</p>
          <ErrorNotice message={actionError} />
          <div className="dialog-actions">
            <button className="button secondary" disabled={busy} onClick={() => setConfirm(null)}>
              Cancel
            </button>
            <button className="button danger" disabled={busy} onClick={() => void action(confirm)}>
              {busy ? 'Working…' : confirm === 'delete' ? 'Delete session' : 'Close session'}
            </button>
          </div>
        </Dialog>
      )}
    </div>
  );
}

function ArrowRightIcon() {
  return <ArrowUp size={13} style={{ transform: 'rotate(90deg)' }} />;
}

function Turn({
  sessionId,
  events,
  index,
  onInspect,
}: {
  sessionId: string;
  events: Event[];
  index: number;
  onInspect: (event: Event) => void;
}) {
  return (
    <section className="turn">
      <div className="turn-divider">
        <span>TURN {index + 1} IN LOADED ACTIVITY</span>
      </div>
      {events.map((event) => {
        const payload = data(event);
        if (event.type === 'user.message')
          return (
            <div className="user-message" key={event.id}>
              <div className="message-label">
                You <time>{timestamp(event.time)}</time>
                <button
                  className="inspect-button"
                  aria-label="Inspect user message"
                  onClick={() => onInspect(event)}
                >
                  <Code2 size={13} />
                </button>
              </div>
              <div className="message-text">{text(payload.text)}</div>
              {Array.isArray(payload.attachments) &&
                payload.attachments.map((reference) => (
                  <a
                    className="attachment-reference"
                    key={String(reference)}
                    href={`/api/brain/sessions/${encodeURIComponent(sessionId)}/attachments?reference=${encodeURIComponent(String(reference))}`}
                    download
                    aria-label={`Download attachment ${String(reference).slice(0, 12)}`}
                  >
                    <Paperclip size={13} />
                    {String(reference).slice(0, 16)}… <Download size={12} />
                  </a>
                ))}
            </div>
          );
        if (event.type === 'assistant.message')
          return (
            <div className="assistant-message" key={event.id}>
              <div className="message-label">
                <span className="agent-avatar">B</span>Brain
                <button
                  className="inspect-button"
                  aria-label="Inspect assistant message"
                  onClick={() => onInspect(event)}
                >
                  <Code2 size={13} />
                </button>
              </div>
              <div className="message-text">{text(payload.text)}</div>
            </div>
          );
        if (event.type === 'tool.call') {
          const result = events.find(
            (item) => item.type === 'tool.result' && data(item).call_id === payload.id,
          );
          const outcome = result
            ? (data(result).result as {
                is_error?: boolean;
                unknown_outcome?: boolean;
                content?: { type: string; text?: string }[];
              })
            : undefined;
          return (
            <details className="tool-call" key={event.id}>
              <summary>
                <Terminal size={15} />
                <strong>{text(payload.name)}</strong>
                <span
                  className={
                    outcome?.is_error || outcome?.unknown_outcome ? 'danger-text' : 'muted'
                  }
                >
                  {outcome?.unknown_outcome
                    ? 'Unknown outcome'
                    : outcome?.is_error
                      ? 'Error'
                      : result
                        ? 'Completed'
                        : 'Awaiting result'}
                </span>
                <ChevronDown size={14} />
              </summary>
              <div className="tool-body">
                <div className="tool-body-label">
                  INPUT{' '}
                  <button className="text-button" onClick={() => onInspect(event)}>
                    Inspect event
                  </button>
                </div>
                <pre>{text(payload.arguments)}</pre>
                {result && (
                  <>
                    <div className="tool-body-label">
                      RESULT{' '}
                      <button className="text-button" onClick={() => onInspect(result)}>
                        Inspect event
                      </button>
                    </div>
                    <pre>
                      {outcome?.content
                        ?.map((item) =>
                          item.type === 'text' ? item.text : `[${item.type} content]`,
                        )
                        .join('\n') ?? text(data(result).result)}
                    </pre>
                  </>
                )}
              </div>
            </details>
          );
        }
        if (event.type === 'session.error')
          return (
            <button
              key={event.id}
              className="notice error error-event"
              onClick={() => onInspect(event)}
            >
              {text(payload.message) || 'Execution stopped. Inspect this event for details.'}
              <Code2 size={14} />
            </button>
          );
        if (
          event.type === 'tool.result' &&
          !events.some((item) => item.type === 'tool.call' && data(item).id === payload.call_id)
        )
          return (
            <button key={event.id} className="event-row" onClick={() => onInspect(event)}>
              <Wrench size={14} />
              Tool result from earlier activity
              <Code2 size={13} />
            </button>
          );
        return null;
      })}
    </section>
  );
}
