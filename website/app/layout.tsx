import { Provider } from '@/components/provider';
import { Header } from '@/components/header';
import type { Metadata } from 'next';
import { assetUrl } from '@/lib/shared';
import './global.css';

export const metadata: Metadata = {
  title: { default: 'Brain documentation', template: '%s · Brain' },
  description: 'A minimal and extensible agent server for building agent applications.',
  icons: { icon: assetUrl('/icon.svg') },
};

export default function Layout({ children }: LayoutProps<'/'>) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <Provider><a className="brain-skip" href="#nd-page">Skip to content</a><Header />{children}</Provider>
      </body>
    </html>
  );
}
