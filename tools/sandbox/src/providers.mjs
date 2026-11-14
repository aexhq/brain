import { ModalClient } from 'modal';
import { Sandbox as E2B } from '@e2b/code-interpreter';
import { Daytona } from '@daytona/sdk';

async function boundedText(stream) {
  let text = '', size = 0;
  for await (const chunk of stream) {
    size += Buffer.byteLength(chunk);
    if (size > 524288) throw new Error('sandbox output exceeds limit');
    text += chunk;
  }
  return text;
}

// Credentials and images are host configuration, never model arguments.
export function providersFromEnv(env = process.env) {
  const providers = {};
  if (env.MODAL_TOKEN_ID || env.MODAL_TOKEN_SECRET) {
    if (!env.MODAL_TOKEN_ID || !env.MODAL_TOKEN_SECRET) throw new Error('both Modal token variables are required');
    const client = new ModalClient({ tokenId: env.MODAL_TOKEN_ID, tokenSecret: env.MODAL_TOKEN_SECRET, timeoutMs: 60000 });
    providers.modal = {
      close() { client.close(); },
      async create() {
        const app = await client.apps.fromName(env.MODAL_APP ?? 'brain-tools', { createIfMissing: true });
        const image = client.images.fromRegistry(env.MODAL_IMAGE ?? 'python:3.13-slim');
        const sb = await client.sandboxes.create(app, image, { timeoutMs: 300000, cpu: 1, memoryMiB: 512, memoryLimitMiB: 512, tags: { service: 'brain-tools' } });
        return sb.sandboxId;
      },
      async exec(id, command) {
        const sb = await client.sandboxes.fromId(id);
        const p = await sb.exec(['/bin/sh', '-lc', command], { timeoutMs: 45000 });
        const [stdout, stderr, code] = await Promise.all([boundedText(p.stdout), boundedText(p.stderr), p.wait()]);
        if (code !== 0) throw new Error(`execution wrapper exited ${code}`);
        if (stderr) throw new Error('execution wrapper reported an error');
        return stdout;
      },
      async inspect(id) { const code = await (await client.sandboxes.fromId(id)).poll(); return { state: code === null ? 'running' : 'terminated', exit_code: code }; },
      async terminate(id) { await (await client.sandboxes.fromId(id)).terminate(); },
    };
  }
  if (env.E2B_API_KEY) {
    const opts = { apiKey: env.E2B_API_KEY, requestTimeoutMs: 60000 };
    providers.e2b = {
      async create() { return (await E2B.create(env.E2B_TEMPLATE ?? 'code-interpreter-v1', { ...opts, timeoutMs: 300000, metadata: { service: 'brain-tools' } })).sandboxId; },
      async exec(id, command) {
        const info = await E2B.getInfo(id, opts);
        if (info.state !== 'running') throw new Error('sandbox is not running');
        const sb = await E2B.connect(id, opts);
        const output = await sb.commands.run(command, { timeoutMs: 45000, requestTimeoutMs: 50000 });
        if (output.exitCode !== 0 || output.stderr) throw new Error('execution wrapper failed');
        return output.stdout;
      },
      async inspect(id) { const info = await E2B.getInfo(id, opts); return { state: info.state, ends_at: info.endAt }; },
      async terminate(id) { await E2B.kill(id, opts); },
    };
  }
  if (env.DAYTONA_API_KEY) {
    const client = new Daytona({ apiKey: env.DAYTONA_API_KEY, apiUrl: env.DAYTONA_API_URL, target: env.DAYTONA_TARGET ?? 'us', requestTimeoutMs: 60000 });
    providers.daytona = {
      async create() {
        const sb = await client.create({ language: 'python', public: false, ephemeral: true, autoStopInterval: 5, autoDeleteInterval: 0, labels: { service: 'brain-tools' } }, { timeout: 60 });
        return sb.id;
      },
      async exec(id, command) {
        const sb = await client.get(id);
        if (sb.state !== 'started') throw new Error('sandbox is not running');
        const output = await sb.process.executeCommand(command, undefined, undefined, 45);
        if (output.exitCode !== 0) throw new Error('execution wrapper failed');
        return output.result;
      },
      async inspect(id) { return { state: (await client.get(id)).state }; },
      async terminate(id) { await client.delete(await client.get(id), 60); },
    };
  }
  if (!Object.keys(providers).length) throw new Error('configure at least one sandbox provider API key');
  return providers;
}
