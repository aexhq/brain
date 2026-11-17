import http from 'node:http';
import { once } from 'node:events';
import { createHash, randomUUID } from 'node:crypto';

export async function fixture() {
  const sessions = new Map();
  const logs = new Map();
  const streams = new Map();
  const accepted = new Map();
  const attachments = new Map();
  const requests = [];
  const revoked = new Set();
  let executions = 0,
    pendingCleanup = false;
  const tool = {
    name: 'lookup_release',
    description: 'Read release details from the connected repository.',
    input_schema: {
      type: 'object',
      properties: { version: { type: 'string' } },
      required: ['version'],
    },
  };
  const seed = (id, status, model, instructions, account = 'a') => {
    const session = {
      id,
      status,
      harness: 'pi',
      config: { model, instructions, tools: [tool.name] },
      descriptors: [tool],
      created_at: '2026-11-15T09:00:00Z',
      updated_at: '2026-11-15T10:23:00Z',
      account,
    };
    sessions.set(id, session);
    logs.set(id, []);
    return session;
  };
  seed(
    'support/issue-42',
    'idle',
    'openai/gpt-4.1-mini',
    'Help customers resolve product questions.',
  );
  seed(
    'release-notes',
    'running',
    'anthropic/claude-sonnet-4-5',
    'Review the latest changes and write release notes.',
  );
  seed(
    'weekly-research',
    'idle',
    'openai/gpt-4.1-mini',
    'Research and summarize the week’s engineering updates.',
  );
  seed(
    'document-review',
    'failed',
    'openai/gpt-4.1-mini',
    'Review documents against the supplied requirements.',
  );
  function publish(id, type, data, input_id, retained = true) {
    const log = logs.get(id);
    const event = {
      id: randomUUID(),
      type,
      data,
      input_id,
      time: new Date().toISOString(),
      ...(retained ? { cursor: `cursor:${log.length + 1}` } : {}),
    };
    if (retained) log.push(event);
    for (const stream of streams.get(id) ?? [])
      stream.write(
        `${event.cursor ? `id: ${event.cursor}\n` : ''}event: ${type}\ndata: ${JSON.stringify(event)}\n\n`,
      );
    return event;
  }
  function status(id, value, input) {
    sessions.get(id).status = value;
    publish(id, 'session.status', { status: value }, input);
  }
  const initial = publish('support/issue-42', 'user.message', {
    text: 'What changed in the latest release?',
  });
  publish(
    'support/issue-42',
    'tool.call',
    { id: 'tool-1', name: tool.name, arguments: { version: 'latest' } },
    initial.id,
  );
  publish(
    'support/issue-42',
    'tool.result',
    {
      call_id: 'tool-1',
      result: {
        content: [
          {
            type: 'text',
            text: 'The release adds session inspection and improved reconnect handling.',
          },
        ],
      },
    },
    initial.id,
  );
  publish(
    'support/issue-42',
    'assistant.message',
    {
      text: 'The latest release adds session inspection and improves reconnect handling. You can now review tool activity and continue from your last event.',
    },
    initial.id,
  );
  publish(
    'support/issue-42',
    'model.request',
    {
      request_id: 'model-1',
      state: 'completed',
      usage: {
        availability: 'reported',
        semantics: 'snapshot',
        input_tokens: 1240,
        output_tokens: 186,
      },
    },
    initial.id,
  );
  status('support/issue-42', 'idle', initial.id);
  const server = http.createServer(async (request, response) => {
    const url = new URL(request.url, 'http://fixture');
    const token = request.headers.authorization?.slice(7);
    const account = token === 'test-b' ? 'b' : 'a';
    const json = (code, body) => {
      response.writeHead(code, {
        'content-type': 'application/json',
        'x-request-id': 'fixture-request',
      });
      response.end(JSON.stringify(body));
    };
    const fail = (code, message) =>
      json(code, { error: { code: code === 409 ? 'conflict' : 'forbidden', message } });
    const bytes = [];
    for await (const chunk of request) bytes.push(chunk);
    const raw = Buffer.concat(bytes);
    const body =
      raw.length && request.headers['content-type']?.includes('application/json')
        ? JSON.parse(raw)
        : undefined;
    requests.push({
      method: request.method,
      path: url.pathname,
      query: url.search,
      token,
      body,
      bytes: raw.length,
    });
    if (!['test-a', 'test-b', 'test-read'].includes(token) || revoked.has(token))
      return fail(401, 'authentication required');
    if (token === 'test-read' && request.method !== 'GET')
      return fail(403, 'credential is read-only');
    if (url.pathname === '/catalog') return json(200, { tools: [tool] });
    if (url.pathname === '/sessions') {
      if (request.method === 'GET') {
        const all = [...sessions.values()]
          .filter(
            (item) => item.account === account && item.id > (url.searchParams.get('after') ?? ''),
          )
          .sort((a, b) => a.id.localeCompare(b.id));
        return json(200, {
          sessions: all
            .slice(0, Number(url.searchParams.get('limit') ?? 100))
            .map(({ account, ...session }) => session),
        });
      }
      if (sessions.has(body.id)) return fail(409, 'session already exists');
      const session = seed(
        body.id,
        'idle',
        body.config.model,
        body.config.instructions ?? '',
        account,
      );
      session.config = body.config;
      session.descriptors = body.config.tools.includes(tool.name) ? [tool] : [];
      const { account: _, ...publicSession } = session;
      return json(201, publicSession);
    }
    const parts = url.pathname.split('/').slice(1).map(decodeURIComponent);
    const id = parts[1],
      operation = parts[2];
    const session = sessions.get(id);
    if (!session || session.account !== account) return fail(404, 'session not found');
    if (request.method === 'DELETE') {
      if (pendingCleanup) {
        pendingCleanup = false;
        return fail(409, 'Resource cleanup is pending. Retry deletion.');
      }
      sessions.delete(id);
      logs.delete(id);
      for (const stream of streams.get(id) ?? []) stream.end();
      return json(200, { status: 'deleted' });
    }
    if (!operation) {
      const { account: _, ...publicSession } = session;
      return json(200, publicSession);
    }
    if (operation === 'close' || operation === 'resume') {
      status(id, operation === 'close' ? 'closed' : 'idle');
      const { account: _, ...publicSession } = session;
      return json(200, operation === 'resume' ? publicSession : { status: session.status });
    }
    if (operation === 'attachments') {
      if (request.method === 'GET') {
        const attachment = attachments.get(`${id}:${url.searchParams.get('reference')}`);
        if (!attachment) return fail(404, 'attachment not found');
        response.writeHead(200, {
          'content-type': attachment.type,
          'content-disposition': 'attachment; filename="notes.txt"',
        });
        response.end(attachment.bytes);
        return;
      }
      const reference = createHash('sha256').update(raw).digest('hex');
      attachments.set(`${id}:${reference}`, { bytes: raw, type: request.headers['content-type'] });
      return json(201, {
        reference,
        name: url.searchParams.get('name'),
        media_type: request.headers['content-type'],
        size: raw.length,
      });
    }
    if (operation === 'events' && request.method === 'GET') {
      const log = logs.get(id);
      const after = Number((url.searchParams.get('after') ?? '').split(':')[1] ?? 0);
      if (request.headers.accept === 'text/event-stream') {
        response.writeHead(200, {
          'content-type': 'text/event-stream',
          'cache-control': 'no-store',
        });
        response.write(': connected\n\n');
        for (const event of log.slice(after))
          response.write(
            `id: ${event.cursor}\nevent: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`,
          );
        const set = streams.get(id) ?? new Set();
        set.add(response);
        streams.set(id, set);
        const timer = setInterval(() => response.write(': keepalive\n\n'), 1000);
        const cleanup = () => {
          clearInterval(timer);
          set.delete(response);
        };
        response.once('finish', cleanup);
        response.once('close', cleanup);
        return;
      }
      const limit = Number(url.searchParams.get('limit') ?? 100);
      const events =
        url.searchParams.get('tail') === 'true'
          ? log.slice(-limit)
          : log.slice(after, after + limit);
      return json(200, {
        events,
        next_cursor: events.at(-1)?.cursor ?? url.searchParams.get('after') ?? '',
      });
    }
    if (operation === 'events' && request.method === 'POST') {
      if (body.type === 'user.interrupt') {
        status(id, 'interrupted');
        return json(202, { status: 'interrupted' });
      }
      const identity = `${id}:${body.key}`;
      if (accepted.has(identity)) return json(202, accepted.get(identity));
      const event = publish(id, 'user.message', body);
      accepted.set(identity, event);
      executions++;
      status(id, 'running', event.id);
      setTimeout(() => {
        if (!sessions.has(id)) return;
        publish(id, 'assistant.delta', { text: 'Working on your task…' }, event.id, false);
        publish(
          id,
          'tool.call',
          { id: event.id + '-tool', name: tool.name, arguments: { version: 'latest' } },
          event.id,
        );
      }, 80);
      setTimeout(() => {
        if (!sessions.has(id)) return;
        publish(
          id,
          'tool.result',
          {
            call_id: event.id + '-tool',
            result: { content: [{ type: 'text', text: 'Release verified.' }] },
          },
          event.id,
        );
        publish(id, 'assistant.message', { text: `Finished: ${body.text}` }, event.id);
        status(id, 'idle', event.id);
      }, 180);
      if (body.text === 'Lose acknowledgement') {
        response.destroy();
        return;
      }
      return json(202, event);
    }
    fail(404, 'not found');
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  return {
    url: `http://127.0.0.1:${server.address().port}`,
    requests,
    revoked,
    sessions,
    logs,
    publish,
    status,
    get executions() {
      return executions;
    },
    pendingCleanup() {
      pendingCleanup = true;
    },
    disconnectStreams(id) {
      const set = streams.get(id);
      if (!set) return;
      for (const stream of set) stream.end();
      set.clear();
    },
    async close() {
      for (const set of streams.values()) for (const stream of set) stream.end();
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
    },
  };
}
