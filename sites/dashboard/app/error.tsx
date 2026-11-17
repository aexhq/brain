'use client';
export default function ErrorPage({ reset }: { reset: () => void }) {
  return (
    <div className="page">
      <h1>Could not open this page</h1>
      <p>Try loading it again. Your agent sessions are unaffected.</p>
      <button className="button secondary" onClick={reset}>
        Try again
      </button>
    </div>
  );
}
