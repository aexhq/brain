import Link from 'next/link';

export default function NotFound() {
  return <main className="brain-not-found"><p>404 / NOT FOUND</p><h1>This page isn’t here.</h1><Link href="/docs">Back to the documentation →</Link></main>;
}
