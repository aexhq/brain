import { llms, loader } from 'fumadocs-core/source';
import { docsRoute } from './shared';
import { openapi } from './openapi';
import { apiMarkdown } from './api-markdown';
import { defineDocs } from 'fumadocs-mdx/macro';
import { metaSchema, pageSchema } from 'fumadocs-core/source/schema';

const docs = defineDocs({
  dir: 'content/docs',
  docs: {
    schema: pageSchema,
    postprocess: {
      includeProcessedMarkdown: true,
    },
  },
  meta: {
    schema: metaSchema,
  },
});

// See https://fumadocs.dev/docs/headless/source-api for more info
export const source = loader({
  baseUrl: docsRoute,
  source: docs.toFumadocsSource(),
  plugins: [openapi.loaderPlugin()],
});

export const docsLlms = llms(source, {
  renderPage: async (page) => `# ${page.data.title} (${page.url})

${page.slugs[0] === 'api' && page.slugs[1] === 'generated' ? apiMarkdown(page.slugs[2]) : await page.data.getText('processed')}`,
});
