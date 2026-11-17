import { NextRequest, NextResponse } from 'next/server';
import { brainOrigin, connectionCookie, failure, readBody, sameOrigin } from '../../../lib/server';

export async function POST(request: NextRequest) {
  if (!sameOrigin(request)) return failure(403, 'Invalid request origin');
  let token: unknown;
  try {
    token = JSON.parse(new TextDecoder().decode(await readBody(request, 2048))).token;
  } catch (error) {
    return failure(error instanceof RangeError ? 413 : 400, 'Enter a valid Brain API token');
  }
  if (
    typeof token !== 'string' ||
    !token ||
    Buffer.byteLength(token) > 1017 ||
    /[\s\x00-\x1f\x7f]/.test(token)
  )
    return failure(400, 'Enter a valid Brain API token');
  try {
    const upstream = await fetch(`${brainOrigin()}/sessions?limit=1`, {
      headers: { authorization: `Bearer ${token}` },
      signal: AbortSignal.timeout(15000),
      cache: 'no-store',
      redirect: 'error',
    });
    await upstream.body?.cancel();
    if (!upstream.ok)
      return failure(
        upstream.status,
        upstream.status === 401 || upstream.status === 403
          ? 'This token cannot list sessions. Use a token with access to this deployment.'
          : 'Brain could not load sessions. Check the service and try again.',
      );
    const response = NextResponse.json(
      { connected: true },
      { headers: { 'cache-control': 'no-store' } },
    );
    response.cookies.set(connectionCookie, token, {
      httpOnly: true,
      secure: new URL(request.headers.get('origin')!).protocol === 'https:',
      sameSite: 'strict',
      path: '/',
      maxAge: 8 * 60 * 60,
    });
    return response;
  } catch {
    return failure(503, 'Cannot reach Brain. Check the configured service address.');
  }
}

export async function DELETE(request: NextRequest) {
  if (!sameOrigin(request)) return failure(403, 'Invalid request origin');
  const response = NextResponse.json(
    { connected: false },
    { headers: { 'cache-control': 'no-store' } },
  );
  response.cookies.delete(connectionCookie);
  return response;
}
