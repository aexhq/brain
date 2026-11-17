'use client';

import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useEffect, useMemo, useState, type FormEvent } from 'react';
import type { Session, Tool } from '@aexhq/brain';
import {
  ArrowLeft,
  ArrowRight,
  Code2,
  ChevronDown,
  Layers,
  Plus,
  RefreshCw,
  Search,
  Wrench,
} from 'lucide-react';
import { useConnection } from './shell';
import { CopyButton, ErrorNotice, Loading, Status, timestamp } from './ui';

export function Sessions() {
  const { brain } = useConnection();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [more, setMore] = useState(false);
  const [cursor, setCursor] = useState('');
  const [search, setSearch] = useState('');
  const [status, setStatus] = useState('all');
  const [sort, setSort] = useState('updated_at');
  async function load(after = '') {
    setLoading(true);
    setError('');
    try {
      const page = await brain.list(after, 50);
      setSessions((current) =>
        after
          ? [
              ...new Map(
                [...current, ...(page.sessions ?? [])].map((item) => [item.id, item]),
              ).values(),
            ]
          : (page.sessions ?? []),
      );
      setCursor(page.sessions?.at(-1)?.id ?? after);
      setMore((page.sessions ?? []).length === 50);
    } catch (error) {
      setError((error as Error).message);
    } finally {
      setLoading(false);
    }
  }
  useEffect(() => {
    void load();
  }, [brain]); // The connection owns the client for this mount.
  const shown = useMemo(
    () =>
      sessions
        .filter(
          (session) =>
            (status === 'all' || session.status === status) &&
            `${session.id} ${session.config.model} ${session.harness}`
              .toLowerCase()
              .includes(search.toLowerCase()),
        )
        .sort((a, b) =>
          sort === 'created_at'
            ? b.created_at.localeCompare(a.created_at)
            : b.updated_at.localeCompare(a.updated_at),
        ),
    [sessions, status, search, sort],
  );
  return (
    <div className="page">
      <div className="page-heading">
        <h1>Sessions</h1>
        <Link className="button primary" href="/sessions/new">
          <Plus size={16} />
          New session
        </Link>
      </div>
      <section className="session-list">
        {(sessions.length > 0 || loading || error) && (
          <div className="table-toolbar">
            <div className="search-field">
              <Search size={17} />
              <input
                aria-label="Filter loaded sessions"
                placeholder="Filter sessions…"
                value={search}
                onChange={(event) => setSearch(event.target.value)}
              />
            </div>
            <div className="toolbar-actions">
              <select
                aria-label="Session status"
                value={status}
                onChange={(event) => setStatus(event.target.value)}
              >
                <option value="all">All statuses</option>
                {['running', 'idle', 'interrupted', 'failed', 'closed'].map((value) => (
                  <option key={value} value={value}>
                    {value[0].toUpperCase() + value.slice(1)}
                  </option>
                ))}
              </select>
              <select
                aria-label="Sort sessions"
                value={sort}
                onChange={(event) => setSort(event.target.value)}
              >
                <option value="updated_at">Recent activity</option>
                <option value="created_at">Newest first</option>
              </select>
              <button
                className="icon-button"
                disabled={loading}
                aria-label="Refresh sessions"
                onClick={() => void load()}
              >
                <RefreshCw size={16} className={loading ? 'spin' : ''} />
              </button>
            </div>
          </div>
        )}
        <ErrorNotice message={error} />
        {loading && !sessions.length ? (
          <Loading label="Loading sessions…" />
        ) : shown.length ? (
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>Session</th>
                  <th>Status</th>
                  <th className="model-column">Model</th>
                  <th className="updated-column">Updated</th>
                </tr>
              </thead>
              <tbody>
                {shown.map((session) => (
                  <tr key={session.id}>
                    <td>
                      <Link
                        className="session-link"
                        href={`/sessions/${encodeURIComponent(session.id)}`}
                      >
                        <div>
                          <strong>{session.id}</strong>
                          {session.config.instructions && (
                            <span>{session.config.instructions}</span>
                          )}
                        </div>
                      </Link>
                    </td>
                    <td>
                      <Status value={session.status} />
                    </td>
                    <td className="model-column">
                      <code className="model-name">{session.config.model}</code>
                    </td>
                    <td className="updated-column muted nowrap">{timestamp(session.updated_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="empty-state">
            <span className="empty-icon">
              <Layers size={26} />
            </span>
            <h2>
              {error
                ? 'Could not load sessions'
                : sessions.length
                  ? 'No matching sessions'
                  : 'No sessions yet'}
            </h2>
            {sessions.length > 0 && <p>Try another filter.</p>}
          </div>
        )}
      </section>
      <div className="list-footer">
        <span className="muted">
          {sessions.length > 0 &&
            `${shown.length === sessions.length ? sessions.length : `${shown.length} of ${sessions.length}`} ${more ? 'loaded ' : ''}sessions`}
        </span>
        {more && (
          <button
            className="button secondary small"
            disabled={loading}
            onClick={() => void load(cursor)}
          >
            Load more
          </button>
        )}
        <details className="open-session">
          <summary>
            Open by ID <ChevronDown size={13} />
          </summary>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              const id = String(new FormData(event.currentTarget).get('id') ?? '').trim();
              if (id) window.location.assign(`/sessions/${encodeURIComponent(id)}`);
            }}
          >
            <input
              name="id"
              aria-label="Session ID to open"
              placeholder="Session ID"
              required
              maxLength={128}
            />
            <button className="button secondary small">
              Open <ArrowRight size={14} />
            </button>
          </form>
        </details>
      </div>
    </div>
  );
}

export function NewSession() {
  const { brain } = useConnection();
  const router = useRouter();
  const [id, setId] = useState('');
  const [model, setModel] = useState('openai/gpt-4.1-mini');
  const [modelKey, setModelKey] = useState('');
  const [instructions, setInstructions] = useState('');
  const [effort, setEffort] = useState('off');
  const [catalog, setCatalog] = useState<Tool[]>([]);
  const [catalogError, setCatalogError] = useState('');
  const [selected, setSelected] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    brain
      .catalog()
      .then((value) => {
        if (active) setCatalog(value.tools ?? []);
      })
      .catch((error) => {
        if (active) setCatalogError(error.message);
      });
    return () => {
      active = false;
    };
  }, [brain]);
  const config = { model: model.trim(), instructions, reasoning_effort: effort, tools: selected };
  const snippet = `import { Brain } from '@aexhq/brain';\n\nconst brain = new Brain(\n  process.env.BRAIN_API_URL,\n  process.env.BRAIN_API_KEY,\n);\n\nconst session = await brain.create({\n  id: ${id ? JSON.stringify(id) : 'crypto.randomUUID()'},\n  harness: 'pi',\n  config: ${JSON.stringify(config, null, 2).replaceAll('\n', '\n  ')},${modelKey ? '\n  model_credential: {\n    api_key: process.env.MODEL_API_KEY,\n  },' : ''}\n});\n\nawait brain.send(session.id, {\n  type: 'user.message',\n  text: 'Hello',\n  key: crypto.randomUUID(),\n});`;
  async function create(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError('');
    const chosen = id || crypto.randomUUID();
    setId(chosen);
    try {
      const session = await brain.create({
        id: chosen,
        harness: 'pi',
        config,
        ...(modelKey ? { model_credential: { api_key: modelKey } } : {}),
      });
      setModelKey('');
      router.push(`/sessions/${encodeURIComponent(session.id)}`);
    } catch (error) {
      setError(
        `${(error as Error).message}. The session ID is kept; check Sessions if creation was not confirmed.`,
      );
      setBusy(false);
    }
  }
  return (
    <div className="page form-page">
      <Link href="/" className="back-link">
        <ArrowLeft size={15} />
        Sessions
      </Link>
      <div className="page-heading">
        <h1>New session</h1>
      </div>
      <form className="config-form" onSubmit={create}>
        <div className="form-section">
          <label htmlFor="model">Model</label>
          <input
            id="model"
            value={model}
            onChange={(event) => setModel(event.target.value)}
            required
            pattern="[^\s/]+/[^\s]+"
            maxLength={256}
          />
          <label htmlFor="model-key">
            Model API key <span className="optional">optional</span>
          </label>
          <input
            id="model-key"
            type="password"
            autoComplete="off"
            value={modelKey}
            onChange={(event) => setModelKey(event.target.value)}
            placeholder="Use deployment credentials"
          />
          <label htmlFor="instructions">
            Instructions <span className="optional">optional</span>
          </label>
          <textarea
            id="instructions"
            rows={5}
            value={instructions}
            onChange={(event) => setInstructions(event.target.value)}
            maxLength={65536}
            placeholder="What should this agent do?"
          />
        </div>
        <div className="form-section">
          <div className="section-title">
            <h2>Tools</h2>
            {selected.length > 0 && <span className="muted">{selected.length} selected</span>}
          </div>
          <ErrorNotice message={catalogError} />
          {catalog.length ? (
            <div className="tool-choices">
              {catalog.map((tool) => (
                <label key={tool.name} className="tool-choice">
                  <input
                    type="checkbox"
                    checked={selected.includes(tool.name)}
                    onChange={(event) =>
                      setSelected((current) =>
                        event.target.checked
                          ? [...current, tool.name]
                          : current.filter((name) => name !== tool.name),
                      )
                    }
                  />
                  <span>
                    <strong>{tool.name}</strong>
                    <small>{tool.description}</small>
                  </span>
                </label>
              ))}
            </div>
          ) : (
            !catalogError && <p className="muted">No tools available.</p>
          )}
        </div>
        <details className="advanced-options">
          <summary>
            Advanced options <ChevronDown size={15} />
          </summary>
          <div className="form-section">
            <label htmlFor="session-id">
              Session ID <span className="optional">optional</span>
            </label>
            <input
              id="session-id"
              value={id}
              onChange={(event) => setId(event.target.value)}
              maxLength={128}
              pattern="(?!\.{1,2}$).*\S.*"
              placeholder="Generated automatically"
            />
            <label htmlFor="effort">Reasoning effort</label>
            <select id="effort" value={effort} onChange={(event) => setEffort(event.target.value)}>
              {['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'].map((value) => (
                <option key={value}>{value}</option>
              ))}
            </select>
            <p className="field-hint">Choose an effort supported by your model.</p>
          </div>
        </details>
        <div className="form-actions">
          <ErrorNotice message={error} />
          <button className="button primary" disabled={busy}>
            {busy ? 'Creating…' : 'Create session'}
            <ArrowRight size={16} />
          </button>
        </div>
      </form>
      <details className="code-preview">
        <summary>
          <span>
            <Code2 size={16} />
            TypeScript example
          </span>
          <ChevronDown size={15} />
        </summary>
        <div className="code-heading">
          <CopyButton value={snippet} label="Copy code" />
        </div>
        <pre>
          <code>{snippet}</code>
        </pre>
      </details>
    </div>
  );
}

export function Tools() {
  const { brain } = useConnection();
  const [tools, setTools] = useState<Tool[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [search, setSearch] = useState('');
  useEffect(() => {
    let active = true;
    brain
      .catalog()
      .then((value) => {
        if (active) setTools(value.tools ?? []);
      })
      .catch((error) => {
        if (active) setError(error.message);
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [brain]);
  const shown = tools.filter((tool) =>
    `${tool.name} ${tool.description}`.toLowerCase().includes(search.toLowerCase()),
  );
  return (
    <div className="page">
      <div className="page-heading">
        <div>
          <h1>
            Tools <span className="heading-count">{tools.length}</span>
          </h1>
        </div>
      </div>
      <div className="search-field tools-search">
        <Search size={17} />
        <input
          aria-label="Filter tools"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          placeholder="Find a tool…"
        />
      </div>
      <ErrorNotice message={error} />
      {loading ? (
        <Loading label="Loading tools…" />
      ) : shown.length ? (
        <div className="tools-grid">
          {shown.map((tool) => (
            <article className="tool-card" key={tool.name}>
              <h2>{tool.name}</h2>
              {tool.description && <p>{tool.description}</p>}
              <details>
                <summary>
                  Input schema <Code2 size={14} />
                </summary>
                <pre>{JSON.stringify(tool.input_schema, null, 2)}</pre>
              </details>
            </article>
          ))}
        </div>
      ) : (
        <div className="empty-state">
          <Wrench size={28} />
          <h2>{search ? 'No matching tools' : 'No tools available'}</h2>
        </div>
      )}
    </div>
  );
}
