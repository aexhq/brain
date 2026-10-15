'use client';

import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { ArrowUpRight } from 'lucide-react';
import { Brand } from './brand';

const links = [
  { href: '/docs', label: 'Documentation', active: (p: string) => !p.startsWith('/docs/api') && !p.startsWith('/docs/sdks') },
  { href: '/docs/api', label: 'API reference', active: (p: string) => p.startsWith('/docs/api') },
  { href: '/docs/sdks/typescript', label: 'SDKs', active: (p: string) => p.startsWith('/docs/sdks') },
];

export function Header() {
  const pathname = usePathname();
  return (
    <header className="brain-header">
      <Link href="/docs" aria-label="Brain documentation" className="brain-header-brand"><Brand /></Link>
      <nav aria-label="Main navigation">
        {links.map(({ href, label, active }) => (
          <Link key={href} href={href} aria-current={active(pathname) ? 'page' : undefined}>{label}</Link>
        ))}
      </nav>
      <a className="brain-github" href="https://github.com/aexhq/brain" target="_blank" rel="noreferrer">GitHub <ArrowUpRight size={14} /></a>
    </header>
  );
}
