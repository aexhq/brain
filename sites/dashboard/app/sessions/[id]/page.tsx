import { SessionWorkspace } from '../../../components/session';
export default async function Page({ params }: { params: Promise<{ id: string }> }) {
  return <SessionWorkspace id={decodeURIComponent((await params).id)} />;
}
