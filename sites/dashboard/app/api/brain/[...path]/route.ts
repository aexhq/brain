import { NextRequest, NextResponse } from 'next/server';
import {
  brainOrigin,
  connectionCookie,
  failure,
  permitted,
  readBody,
  sameOrigin,
} from '../../../../lib/server';

export const dynamic = 'force-dynamic';

async function proxy(request: NextRequest) {
  let path: string[];
  try {
    path = request.nextUrl.pathname.slice('/api/brain/'.length).split('/').map(decodeURIComponent);
  } catch {
    return failure(400, 'Invalid request path');
  }
  if (!permitted(request.method, path, request.nextUrl.searchParams))
    return failure(404, 'Not found');
  if (request.method !== 'GET' && !sameOrigin(request))
    return failure(403, 'Invalid request origin');
  const token = request.cookies.get(connectionCookie)?.value;
  if (!token) return failure(401, 'Connect to Brain to continue');
  const streaming =
    request.method === 'GET' &&
    path[2] === 'events' &&
    request.headers.get('accept')?.includes('text/event-stream');
  const headers = new Headers({
    authorization: `Bearer ${token}`,
    accept: streaming ? 'text/event-stream' : 'application/json',
  });
  if (request.headers.has('content-type'))
    headers.set('content-type', request.headers.get('content-type')!);
  try {
    const body =
      request.method === 'POST'
        ? await readBody(request, path[2] === 'attachments' ? 16 << 20 : 80 << 10)
        : undefined;
    const upstream = await fetch(
      `${brainOrigin()}/${path.map(encodeURIComponent).join('/')}${request.nextUrl.search}`,
      {
        method: request.method,
        headers,
        body,
        redirect: 'error',
        cache: 'no-store',
        signal: streaming
          ? request.signal
          : AbortSignal.any([request.signal, AbortSignal.timeout(30000)]),
      },
    );
    const response = new NextResponse(upstream.body, {
      status: upstream.status,
      headers: {
        'content-type': upstream.headers.get('content-type') ?? 'application/json',
        'cache-control': 'no-store',
        'x-content-type-options': 'nosniff',
        ...(request.method === 'GET' && path[2] === 'attachments' && upstream.ok
          ? { 'content-disposition': upstream.headers.get('content-disposition') ?? 'attachment' }
          : {}),
        ...(upstream.headers.has('x-request-id')
          ? { 'x-request-id': upstream.headers.get('x-request-id')! }
          : {}),
        ...(streaming ? { 'x-accel-buffering': 'no' } : {}),
      },
    });
    if (upstream.status === 401) response.cookies.delete(connectionCookie);
    return response;
  } catch (error) {
    if (error instanceof RangeError) return failure(413, error.message);
    return failure(
      503,
      'Brain could not confirm the request. Check its state before trying again.',
    );
  }
}

export { proxy as GET, proxy as POST, proxy as DELETE };
