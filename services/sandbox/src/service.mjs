import { createHmac, timingSafeEqual } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { failure, result } from '../../http.mjs';

const script = Buffer.from(readFileSync(new URL('./execute.py', import.meta.url))).toString('base64');
export function executionCommand(command) {
  return `python3 -c "import base64;exec(base64.b64decode('${script}'))" '${Buffer.from(command).toString('base64')}'`;
}

export function sandboxService({ providers, signingKey, now = Date.now }) {
  if (!signingKey || signingKey.length < 32) throw new Error('SANDBOX_SIGNING_KEY must contain at least 32 characters');
  const sign = text => createHmac('sha256', signingKey).update(text).digest('base64url');
  function handle(value) {
    const data = Buffer.from(JSON.stringify(value)).toString('base64url');
    return `${data}.${sign(data)}`;
  }
  function readHandle(raw, scope) {
    if (typeof raw !== 'string' || raw.length > 4096) throw new Error('invalid sandbox handle');
    const [data, signature, extra] = raw.split('.');
    const expected = Buffer.from(sign(data)), supplied = Buffer.from(signature ?? '');
    if (extra || expected.length !== supplied.length || !timingSafeEqual(expected, supplied)) throw new Error('invalid sandbox handle');
    const value = JSON.parse(Buffer.from(data, 'base64url'));
    if (value.tenant !== scope.tenant_id || value.session !== scope.session_id) throw new Error('sandbox belongs to another session');
    return value;
  }
  return async (path, request, signal) => {
    const args = request.arguments;
    let value, provider;
    try {
      const fields = path === '/create' ? ['provider'] : path === '/exec' ? ['sandbox', 'command'] : ['/inspect', '/terminate'].includes(path) ? ['sandbox'] : null;
      if (!fields || Object.keys(args).some(k => !fields.includes(k)) || fields.some(k => typeof args[k] !== 'string' || !args[k])) throw new Error('invalid sandbox arguments');
      if (path === '/create') {
        if (!Object.hasOwn(providers, args.provider)) throw new Error('sandbox provider is not configured');
        provider = providers[args.provider];
      } else {
        value = readHandle(args.sandbox, request);
        provider = providers[value.provider];
        if (!provider) throw new Error('sandbox provider is not configured');
        if (path === '/exec' && (now() >= value.expires || args.command.length > 16384)) throw new Error('sandbox expired or command exceeds limit');
      }
    } catch (error) { return failure(error.message); }
    if (signal.aborted) return failure('request cancelled before dispatch');
    try {
      if (path === '/create') {
        const id = await provider.create();
        const expires = now() + 300_000;
        if (signal.aborted) {
          try { await provider.terminate(id); } catch { console.error(JSON.stringify({ event: 'sandbox_cleanup_failed', provider: args.provider, sandbox_id: id })); }
          return failure('request cancelled during sandbox creation', true);
        }
        return result({ sandbox: handle({ id, provider: args.provider, tenant: request.tenant_id, session: request.session_id, expires }), expires_at: new Date(expires).toISOString() });
      }
      if (path === '/inspect') return result(await provider.inspect(value.id));
      if (path === '/terminate') { await provider.terminate(value.id); return result({ terminated: true }); }
      const output = await provider.exec(value.id, executionCommand(args.command));
      if (signal.aborted) return failure('execution response lost after cancellation; inspect before retrying', true);
      if (typeof output !== 'string' || Buffer.byteLength(output) > 524288) throw new Error('invalid execution output');
      const data = JSON.parse(output);
      if (!Number.isInteger(data.exit_code) || typeof data.stdout !== 'string' || typeof data.stderr !== 'string' || typeof data.timed_out !== 'boolean' || typeof data.truncated !== 'boolean') throw new Error('invalid execution result');
      return result(data);
    } catch (error) {
      console.error(JSON.stringify({ event: 'sandbox_provider_error', operation: path, provider: value?.provider ?? args.provider, error: error.name }));
      return failure('sandbox provider operation failed; remote outcome may be unknown', true);
    }
  };
}
