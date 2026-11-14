#!/usr/bin/env node
import { parseArgs } from 'node:util';
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { Brain, Pi, Model, ApiKey, configureTelemetry, telemetry } from './index.js';

export { configureTelemetry, telemetry };

/** The CLI uses the same public SDK and telemetry configuration as applications. */
export async function main(args = process.argv.slice(2), options = {}) {
  const { values, positionals } = parseArgs({ args, allowPositionals: true, options: {
    url: { type: 'string', default: process.env.BRAIN_URL ?? 'http://127.0.0.1:8080' },
    id: { type: 'string' }, model: { type: 'string' }, file: { type: 'string' },
    'content-type': { type: 'string', default: 'application/octet-stream' },
    'credential-env': { type: 'string' }, help: { type: 'boolean' },
  } });
  const [command, id, ...text] = positionals;
  const output = options.output ?? (value => console.log(typeof value === 'string' ? value : JSON.stringify(value)));
  if (values.help || !command) { output('brain create --id ID --model PROVIDER/MODEL [--credential-env KEY_ENV]\nbrain get|history|observe|interrupt|resume|close|delete|flush|sync ID\nbrain list\nbrain send ID MESSAGE\nbrain attach ID --file PATH [--content-type TYPE]\nbrain upload-tool --file PACKAGE.json\nSet BRAIN_API_KEY and optionally BRAIN_URL.'); return; }
  const token = process.env.BRAIN_API_KEY;
  if (!options.client && !token) throw new Error('BRAIN_API_KEY is required');
  const client = options.client ?? new Brain(values.url, token);
  try {
    if (command === 'create') {
      if (!values.id || !values.model) throw new Error('create requires --id and --model');
      const key = values['credential-env'] && process.env[values['credential-env']];
      if (values['credential-env'] && !key) throw new Error('credential environment is empty');
      output(await client.create({ id: values.id, harness: Pi(), model: Model(values.model, key ? ApiKey(key) : undefined) }));
    } else if (command === 'list') output(await client.list());
    else if (command === 'upload-tool') {
      if (!values.file) throw new Error('upload-tool requires --file');
      const pkg = JSON.parse(await readFile(values.file, 'utf8')); output(await client.uploadTool(pkg.tool, pkg.module));
    } else {
      if (!id) throw new Error('session ID is required');
      if (command === 'send') { if (!text.length) throw new Error('message is required'); output(await client.send(id, { type: 'user.message', text: text.join(' ') })); }
      else if (command === 'observe') { for await (const event of client.observe(id)) output(event); }
      else if (command === 'attach') {
        if (!values.file) throw new Error('attach requires --file');
        const { openAsBlob } = await import('node:fs'); const { basename } = await import('node:path');
        output(await client.uploadAttachment(id, basename(values.file), await openAsBlob(values.file, { type: values['content-type'] })));
      } else if (['get', 'history', 'interrupt', 'resume', 'close', 'delete', 'flush', 'sync'].includes(command)) output((await client[command](id)) ?? { status: 'ok' });
      else throw new Error(`Unknown command: ${command}`);
    }
  } finally { await client.dispose(); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main().catch(error => { console.error(error.message); process.exitCode = 1; });
