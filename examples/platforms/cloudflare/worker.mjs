const reply = (value, error = false) => Response.json({ content: [{ type: 'text', text: JSON.stringify(value) }], is_error: error });

export default {
  async fetch(request, env) {
    if (!env.TOOL_API_KEY || env.TOOL_API_KEY.length < 24) return new Response('tool credential missing', { status: 503 });
    const supplied = new TextEncoder().encode(request.headers.get('authorization') ?? '');
    const expected = new TextEncoder().encode(`Bearer ${env.TOOL_API_KEY}`);
    let different = supplied.length ^ expected.length;
    for (let i = 0; i < expected.length; i++) different |= expected[i] ^ (supplied[i] ?? 0);
    if (different) return new Response('unauthorized', { status: 401 });
    if (request.method !== 'POST') return new Response('method not allowed', { status: 405 });
    const reader = request.body?.getReader();
    if (!reader) return new Response('request required', { status: 400 });
    const chunks = []; let length = 0;
    while (true) { const { done, value } = await reader.read(); if (done) break; length += value.length; if (length > 65536) { await reader.cancel(); return new Response('request too large', { status: 413 }); } chunks.push(value); }
    let body;
    try { const bytes = new Uint8Array(length); let offset = 0; for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; } body = JSON.parse(new TextDecoder().decode(bytes)); } catch { return new Response('invalid JSON', { status: 400 }); }
    if (!body || typeof body.tenant_id !== 'string' || !body.tenant_id || body.tenant_id.length > 256 || typeof body.session_id !== 'string' || !body.session_id || typeof body.call_id !== 'string' || !body.call_id) return new Response('identity required', { status: 400 });
    const args = body.arguments, operation = new URL(request.url).pathname;
    if (!['/read', '/write'].includes(operation) || !args || typeof args.key !== 'string' || !/^[a-zA-Z0-9_.-]{1,128}$/.test(args.key) || args.key === '..' || Object.keys(args).some(k => !['key', 'text'].includes(k))) return reply({ error: 'invalid operation or object key' }, true);
    const tenant = btoa(String.fromCharCode(...new TextEncoder().encode(body.tenant_id))).replaceAll('/', '_').replaceAll('+', '-').replaceAll('=', '');
    const key = `tools/${tenant}/${args.key}`;
    if (operation === '/write') {
      if (env.ALLOW_WRITE !== 'true' || typeof args.text !== 'string') return reply({ error: 'writes disabled or text missing' }, true);
      try { await env.BUCKET.put(key, args.text); return reply({ key: args.key, written: true }); }
      catch { return Response.json({ content: [{ type: 'text', text: 'write failed; remote outcome may be unknown' }], is_error: true, unknown_outcome: true }); }
    }
    if (args.text !== undefined) return reply({ error: 'unexpected text argument' }, true);
    try {
      const object = await env.BUCKET.get(key);
      if (!object) return reply({ error: 'object not found' }, true);
      if (object.size > 65536) return reply({ error: 'object exceeds 64 KiB' }, true);
      return reply({ key: args.key, text: await object.text() });
    } catch { return reply({ error: 'object read failed' }, true); }
  },
};
