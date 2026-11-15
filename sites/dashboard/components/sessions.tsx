'use client';

import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useEffect, useMemo, useState, type FormEvent } from 'react';
import type { Session, Tool } from '@aexhq/brain';
import {
  ArrowLeft,
  ArrowRight,
  ArrowUpRight,
  Code2,
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
        <div>
          <span className="eyebrow">YOUR AGENTS AT WORK</span>
          <h1>Sessions</h1>
          <p>Start a task. Follow the work. Keep the conversation going.</p>
        </div>
        <Link className="button primary" href="/sessions/new">
          <Plus size={17} />
          New session
        </Link>
      </div>
      <div className="stats">
        <div>
          <span>Loaded sessions</span>
          <strong>{sessions.length.toLocaleString()}</strong>
          <span className="stat-note">Saved conversations</span>
        </div>
        <div>
          <span>
            <i className="dot running" />
            Running
          </span>
          <strong>{sessions.filter((item) => item.status === 'running').length}</strong>
          <span className="stat-note">Agents working now</span>
        </div>
        <div>
          <span>
            <i className="dot attention" />
            Needs attention
          </span>
          <strong>
            {sessions.filter((item) => ['failed', 'interrupted'].includes(item.status)).length}
          </strong>
          <span className="stat-note">Review and resume</span>
        </div>
      </div>
      <section className="panel">
        <div className="table-toolbar">
          <div className="search-field">
            <Search size={17} />
            <input
              aria-label="Filter loaded sessions"
              placeholder="Filter by ID, model, or runtime…"
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
                  <th>Model</th>
                  <th>Runtime</th>
                  <th>Last activity</th>
                  <th>
                    <span className="sr-only">Open</span>
                  </th>
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
                        <span className="session-glyph">
                          <Layers size={16} />
                        </span>
                        <div>
                          <strong>{session.id}</strong>
                          <span>{session.config.instructions || 'Agent session'}</span>
                        </div>
                      </Link>
                    </td>
                    <td>
                      <Status value={session.status} />
                    </td>
                    <td>
                      <span className="model-name">
                        {session.config.model.split('/').slice(1).join('/') || session.config.model}
                      </span>
                      <span className="cell-subtitle">{session.config.model.split('/')[0]}</span>
                    </td>
                    <td>
                      <code className="runtime-pill">{session.harness}</code>
                    </td>
                    <td className="nowrap muted">{timestamp(session.updated_at)}</td>
                    <td>
                      <Link
                        className="icon-button"
                        href={`/sessions/${encodeURIComponent(session.id)}`}
                        aria-label={`Open session ${session.id}`}
                      >
                        <ArrowUpRight size={16} />
                      </Link>
                    </td>
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
            <h2>{sessions.length ? 'No matching sessions' : 'Your first session starts here'}</h2>
            <p>
              {sessions.length
                ? 'Try another filter, or load more sessions.'
                : 'Give an agent a task and follow its progress, tools, and results in one place.'}
            </p>
            {!sessions.length && !error && (
              <Link href="/sessions/new" className="button primary">
                <Plus size={16} />
                Create a session
              </Link>
            )}
          </div>
        )}
        <div className="table-footer">
          <span>
            {shown.length} of {sessions.length} loaded sessions{more ? ' · More available' : ''}
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
          <span className="muted">Filters apply to loaded sessions</span>
        </div>
      </section>
      <form
        className="open-session"
        onSubmit={(event) => {
          event.preventDefault();
          const id = String(new FormData(event.currentTarget).get('id') ?? '').trim();
          if (id) window.location.assign(`/sessions/${encodeURIComponent(id)}`);
        }}
      >
        <span>Have a session ID?</span>
        <input
          name="id"
          aria-label="Session ID to open"
          placeholder="Open a specific session"
          required
          maxLength={128}
        />
        <button className="text-button">
          Open <ArrowRight size={14} />
        </button>
      </form>
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
    <div className="page">
      <Link href="/" className="back-link">
        <ArrowLeft size={15} />
        Sessions
      </Link>
      <div className="page-heading">
        <div>
          <span className="eyebrow">START SOMETHING</span>
          <h1>New session</h1>
          <p>Choose how your agent works. Send its first task once you’re ready.</p>
        </div>
        <span className="runtime-pill">Pi runtime</span>
      </div>
      <div className="creation-grid">
        <form className="panel config-form" onSubmit={create}>
          <div className="form-section">
            <h2>Configuration</h2>
            <label htmlFor="session-id">
              Session ID <span className="optional">optional</span>
            </label>
            <input
              id="session-id"
              value={id}
              onChange={(event) => setId(event.target.value)}
              maxLength={128}
              pattern="(?!\.{1,2}$).*\S.*"
              placeholder="Generated automatically, or use your own"
            />
            <label htmlFor="model">Model</label>
            <input
              id="model"
              value={model}
              onChange={(event) => setModel(event.target.value)}
              required
              pattern="[^\s/]+/[^\s]+"
              maxLength={256}
            />
            <p className="field-hint">Use a provider/model supported by your deployment.</p>
            <label htmlFor="model-key">
              Model API key <span className="optional">optional</span>
            </label>
            <input
              id="model-key"
              type="password"
              autoComplete="off"
              value={modelKey}
              onChange={(event) => setModelKey(event.target.value)}
              placeholder="Use the deployment’s credential, or supply yours"
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
              placeholder="What should this agent do, and how should it approach its work?"
            />
            <label htmlFor="effort">Reasoning effort</label>
            <select id="effort" value={effort} onChange={(event) => setEffort(event.target.value)}>
              {['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'].map((value) => (
                <option key={value}>{value}</option>
              ))}
            </select>
            <p className="field-hint">Choose an effort supported by the selected model.</p>
          </div>
          <div className="form-section">
            <div className="section-title">
              <h2>Tools</h2>
              <span className="count">{selected.length} selected</span>
            </div>
            <p className="muted">Only tools available to your token are listed.</p>
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
              !catalogError && <p className="muted">No tools are available in this deployment.</p>
            )}
          </div>
          <div className="form-section">
            <ErrorNotice message={error} />
            <button className="button primary" disabled={busy}>
              {busy ? 'Creating…' : 'Create session'}
              <ArrowRight size={16} />
            </button>
          </div>
        </form>
        <aside className="code-preview">
          <div className="code-heading">
            <span>
              <Code2 size={16} />
              Use it in your app
            </span>
            <CopyButton value={snippet} label="Copy code" />
          </div>
          <pre>
            <code>{snippet}</code>
          </pre>
          <p>
            The same configuration, through the TypeScript SDK. Credentials use environment
            variables.
          </p>
        </aside>
      </div>
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
          <span className="eyebrow">WHAT YOUR AGENTS CAN DO</span>
          <h1>
            Tools <span className="heading-count">{tools.length}</span>
          </h1>
          <p>Explore the capabilities available to your workspace.</p>
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
            <article className="panel tool-card" key={tool.name}>
              <span className="tool-icon">
                <Wrench size={20} />
              </span>
              <h2>{tool.name}</h2>
              <p>{tool.description || 'Available in this deployment.'}</p>
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
          <p>
            {search
              ? 'Try a different search.'
              : 'Tools configured for your tenant will appear here.'}
          </p>
        </div>
      )}
    </div>
  );
}
