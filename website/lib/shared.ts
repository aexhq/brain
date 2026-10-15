import { createGetUrl } from 'fumadocs-core/source';

export const appName = 'Brain';
export const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? '';
export const assetUrl = (path: string) => `${basePath}${path}`;
export const markdownLinks = (text: string) => text.replace(/\]\(\/(?!\/)/g, `](${basePath}/`);
export const docsRoute = '/docs';
export const docsContentRoute = '/llms.mdx/docs';

export const gitConfig = {
  user: 'aexhq',
  repo: 'brain',
  branch: 'main',
};

const getContentUrl = createGetUrl(docsContentRoute);

export function getPageMarkdownUrl(page: { slugs: string[]; locale?: string }) {
  const segments = [...page.slugs, 'content.md'];

  return { segments, url: assetUrl(getContentUrl(segments, page.locale)) };
}
