'use client';

import Link from 'next/link';
import { usePathname, useRouter } from 'next/navigation';
import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type FormEvent,
  type ReactNode,
} from 'react';
import type { Brain } from '@aexhq/brain';
import {
  ArrowRight,
  BookOpen,
  ExternalLink,
  KeyRound,
  Layers,
  LogOut,
  Menu,
  Settings2,
  Wrench,
  X,
} from 'lucide-react';
import { client } from '../lib/client';
import { ErrorNotice, Loading } from './ui';

const Connection = createContext<{
  brain: Brain;
  endpoint: string;
  disconnect: () => Promise<void>;
} | null>(null);
export function useConnection() {
  const context = useContext(Connection);
  if (!context) throw new Error('Missing dashboard connection');
  return context;
}

function Brand() {
  return (
    <Link className="brand" href="/" aria-label="Brain Console home">
      <span className="brand-icon">
        <svg width="22" height="22" viewBox="0 0 25 25" fill="none" aria-hidden="true">
          <path d="M4 3h10l6 6-4 4 5 5-4 4H4V3Z" stroke="currentColor" strokeWidth="2" />
          <path d="M9 8h5m-5 5h6m-6 5h7M9 7v12" stroke="currentColor" strokeWidth="2" />
        </svg>
      </span>
      <strong>
        brain<span className="brand-period">.</span>
      </strong>
    </Link>
  );
}

export function Shell({
  initiallyConnected,
  endpoint,
  children,
}: {
  initiallyConnected: boolean;
  endpoint: string;
  children: ReactNode;
}) {
  const [connected, setConnected] = useState(initiallyConnected);
  const [mobile, setMobile] = useState(false);
  const [ready, setReady] = useState(false);
  useEffect(() => setReady(true), []);
  const pathname = usePathname();
  const router = useRouter();
  const brain = useMemo(() => client(() => setConnected(false)), []);
  async function disconnect() {
    const response = await fetch('/api/connection', { method: 'DELETE' });
    if (!response.ok) throw new Error('Could not disconnect. Try again.');
    setConnected(false);
    router.refresh();
  }
  // Forms must not accept input before React attaches their event handlers.
  if (!ready) return <Loading label="Opening workspace…" />;
  if (!connected)
    return (
      <div className="connect-page">
        <div className="connect-top">
          <Brand />
          <a href="https://aexhq.github.io/brain/docs/" target="_blank" rel="noreferrer">
            Documentation <ExternalLink size={14} />
          </a>
        </div>
        <Connect
          endpoint={endpoint}
          onConnect={() => {
            setConnected(true);
            router.refresh();
          }}
        />
      </div>
    );
  const links = [
    { href: '/', label: 'Sessions', icon: Layers },
    { href: '/tools', label: 'Tools', icon: Wrench },
    { href: '/settings', label: 'Connection', icon: Settings2 },
  ];
  return (
    <Connection.Provider value={{ brain, endpoint, disconnect }}>
      <div className="app-shell">
        <a href="#main" className="skip-link">
          Skip to content
        </a>
        {mobile && (
          <button
            className="nav-overlay"
            onClick={() => setMobile(false)}
            aria-label="Close navigation"
          />
        )}
        <aside className={`sidebar ${mobile ? 'sidebar-open' : ''}`}>
          <div className="sidebar-brand">
            <Brand />
            <button
              className="icon-button mobile-only"
              onClick={() => setMobile(false)}
              aria-label="Close navigation"
            >
              <X size={18} />
            </button>
          </div>
          <nav aria-label="Main navigation">
            {links.map(({ href, label, icon: Icon }) => (
              <Link
                key={href}
                href={href}
                onClick={() => setMobile(false)}
                aria-current={
                  (
                    href === '/'
                      ? pathname === '/' || pathname.startsWith('/sessions')
                      : pathname === href
                  )
                    ? 'page'
                    : undefined
                }
              >
                <Icon size={17} />
                {label}
              </Link>
            ))}
          </nav>
          <div className="sidebar-bottom">
            <a href="https://aexhq.github.io/brain/docs/" target="_blank" rel="noreferrer">
              <BookOpen size={17} />
              Documentation
              <ExternalLink size={13} />
            </a>
          </div>
        </aside>
        <div className="main-column">
          <header className="mobile-header">
            <button
              className="icon-button"
              onClick={() => setMobile(true)}
              aria-label="Open navigation"
            >
              <Menu size={20} />
            </button>
            <Brand />
          </header>
          <main id="main">{children}</main>
        </div>
      </div>
    </Connection.Provider>
  );
}

function Connect({ endpoint, onConnect }: { endpoint: string; onConnect: () => void }) {
  const [token, setToken] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError('');
    try {
      const response = await fetch('/api/connection', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ token }),
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error?.message ?? 'Could not connect');
      setToken('');
      onConnect();
    } catch (error) {
      setError((error as Error).message);
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="connect-card">
      <h1>Connect to Brain</h1>
      <p className="connect-endpoint">
        <code>{endpoint}</code>
      </p>
      <form onSubmit={submit}>
        <label htmlFor="token">Brain API token</label>
        <div className="input-icon">
          <KeyRound size={17} />
          <input
            id="token"
            type="password"
            autoComplete="off"
            value={token}
            onChange={(event) => setToken(event.target.value)}
            required
            maxLength={1017}
            placeholder="Deployment token"
          />
        </div>
        <ErrorNotice message={error} />
        <button className="button primary full" disabled={busy} aria-label="Connect to Brain">
          {busy ? 'Connecting…' : 'Connect'}
          <ArrowRight size={16} />
        </button>
      </form>
      <a
        className="connect-note"
        href="https://aexhq.github.io/brain/docs/guides/authentication/"
        target="_blank"
        rel="noreferrer"
      >
        Token setup <ExternalLink size={12} />
      </a>
    </div>
  );
}

export function ConnectionSettings() {
  const { endpoint, disconnect } = useConnection();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  return (
    <div className="page">
      <div className="page-heading">
        <div>
          <h1>Connection</h1>
        </div>
      </div>
      <section className="panel settings-panel">
        <dl className="details">
          <div>
            <dt>Service address</dt>
            <dd>
              <code>{endpoint}</code>
            </dd>
          </div>
          <div>
            <dt>Authentication</dt>
            <dd>Scoped API token</dd>
          </div>
          <div>
            <dt>Browser access</dt>
            <dd>Expires after 8 hours</dd>
          </div>
        </dl>
        <p className="muted">Disconnecting leaves your agent sessions running.</p>
        <ErrorNotice message={error} />
        <button
          disabled={busy}
          className="button secondary"
          onClick={async () => {
            setBusy(true);
            try {
              await disconnect();
            } catch (error) {
              setError((error as Error).message);
              setBusy(false);
            }
          }}
        >
          <LogOut size={16} />
          Disconnect
        </button>
      </section>
    </div>
  );
}
