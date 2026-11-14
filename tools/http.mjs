import http from 'node:http';
import { timingSafeEqual } from 'node:crypto';

export const result = (value) => ({ content: [{ type: 'text', text: JSON.stringify(value) }] });
export const failure = (message, unknown = false) => ({ content: [{ type: 'text', text: message }], is_error: true, unknown_outcome: unknown });

// Only authenticated Brain hosts may supply identity. Model arguments never do.
export function toolServer({ token, invoke, capacity = 16 }) {
  if (!token || token.length < 24) throw new Error('TOOL_API_KEY must contain at least 24 characters');
  let active = 0;
  return http.createServer(async (req, res) => {
    const send = (status, body) => { if (!res.destroyed) { res.writeHead(status, { 'content-type': 'application/json' }); res.end(JSON.stringify(body)); } };
    if (req.method === 'GET' && req.url === '/health') return send(200, { status: 'ok' });
    const supplied = Buffer.from(req.headers.authorization ?? ''), expected = Buffer.from(`Bearer ${token}`);
    if (supplied.length !== expected.length || !timingSafeEqual(supplied, expected)) return send(401, { error: 'unauthorized' });
    if (req.method !== 'POST') return send(405, { error: 'method_not_allowed' });
    if (active >= capacity) return send(200, failure('tool service is at capacity; no action dispatched'));
    active++;
    const controller = new AbortController();
    res.on('close', () => controller.abort());
    const timer = setTimeout(() => controller.abort(), 90_000);
    try {
      const chunks = []; let bytes = 0;
      for await (const chunk of req) {
        bytes += chunk.length;
        if (bytes > 131072) { send(413, { error: 'request_too_large' }); return; }
        chunks.push(chunk);
      }
      let body;
      try { body = JSON.parse(Buffer.concat(chunks)); } catch { return send(400, { error: 'invalid_json' }); }
      if (!body || typeof body !== 'object' || !['tenant_id', 'session_id', 'call_id'].every(k => typeof body[k] === 'string' && body[k].length > 0 && body[k].length <= 256) || !body.arguments || typeof body.arguments !== 'object' || Array.isArray(body.arguments)) return send(400, { error: 'invalid_request' });
      send(200, await invoke(req.url, body, controller.signal));
    } catch { send(200, failure('tool request failed; remote outcome may be unknown', true)); }
    finally { clearTimeout(timer); active--; }
  });
}
