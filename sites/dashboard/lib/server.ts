import { NextResponse } from 'next/server';

export const connectionCookie = 'brain_dashboard';

export function brainOrigin() {
  const url = new URL(process.env.BRAIN_API_URL ?? 'http://127.0.0.1:8080');
  if (
    !['http:', 'https:'].includes(url.protocol) ||
    url.username ||
    url.password ||
    url.pathname !== '/' ||
    url.search ||
    url.hash
  ) {
    throw new Error('BRAIN_API_URL must be an HTTP(S) origin without credentials or a path');
  }
  return url.origin;
}

export function failure(status: number, message: string, code = 'dashboard_error') {
  return NextResponse.json(
    { error: { code, message } },
    { status, headers: { 'cache-control': 'no-store' } },
  );
}

export function sameOrigin(request: Request) {
  const origin = request.headers.get('origin');
  if (!origin) return false;
  try {
    const url = new URL(origin);
    return ['http:', 'https:'].includes(url.protocol) && url.host === request.headers.get('host');
  } catch {
    return false;
  }
}

export async function readBody(request: Request, limit: number) {
  const reader = request.body?.getReader();
  if (!reader) return new Uint8Array();
  const chunks: Uint8Array[] = [];
  let size = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.length;
      if (size > limit) {
        await reader.cancel();
        throw new RangeError('Request body exceeds limit');
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }
  return Buffer.concat(chunks);
}

// The proxy is a browser boundary, not an unrestricted upstream HTTP client.
export function permitted(method: string, path: string[], query: URLSearchParams) {
  let keys: string[] = [];
  let allowed = false;
  if (path.length === 1 && path[0] === 'catalog') allowed = method === 'GET';
  if (path.length === 1 && path[0] === 'sessions') {
    allowed = method === 'GET' || method === 'POST';
    if (method === 'GET') keys = ['after', 'limit'];
  }
  if (
    path[0] === 'sessions' &&
    path.length >= 2 &&
    Buffer.byteLength(path[1]) <= 128 &&
    path[1].trim() &&
    !/[\x00-\x1f\x7f]/.test(path[1]) &&
    !['.', '..'].includes(path[1])
  ) {
    if (path.length === 2) allowed = method === 'GET' || method === 'DELETE';
    if (path.length === 3) {
      if (path[2] === 'events') {
        allowed = method === 'GET' || method === 'POST';
        if (method === 'GET') keys = ['after', 'limit', 'tail'];
      }
      if (['resume', 'close'].includes(path[2])) allowed = method === 'POST';
      if (path[2] === 'attachments') {
        allowed = method === 'GET' || method === 'POST';
        keys = method === 'GET' ? ['reference'] : ['name'];
      }
    }
  }
  if (
    !allowed ||
    [...query.keys()].some((key) => !keys.includes(key) || query.getAll(key).length !== 1)
  )
    return false;
  if (query.has('limit') && !/^(?:[1-9]\d{0,2}|1000)$/.test(query.get('limit')!)) return false;
  if (query.has('tail') && query.get('tail') !== 'true') return false;
  if (Buffer.byteLength(query.get('after') ?? '') > 512) return false;
  return true;
}
