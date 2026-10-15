import { docsLlms } from '@/lib/source';
import { markdownLinks } from '@/lib/shared';

export const revalidate = false;

export async function GET() {
  return new Response(markdownLinks(await docsLlms.index()));
}
