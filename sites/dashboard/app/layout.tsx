import type { Metadata } from 'next';
import { cookies } from 'next/headers';
import { Shell } from '../components/shell';
import { brainOrigin, connectionCookie } from '../lib/server';
import './globals.css';

export const metadata: Metadata = {
  title: 'Brain Console',
  description: 'Run and inspect your Brain agent sessions.',
};

export default async function Layout({ children }: { children: React.ReactNode }) {
  const connected = (await cookies()).has(connectionCookie);
  return (
    <html lang="en">
      <body>
        <Shell initiallyConnected={connected} endpoint={brainOrigin()}>
          {children}
        </Shell>
      </body>
    </html>
  );
}
