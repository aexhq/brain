#!/usr/bin/env node
import { Command } from 'commander';
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { Brain, Pi, Model, ApiKey, configureTelemetry, telemetry } from './index.js';

export { configureTelemetry, telemetry };

/** The CLI uses the same public SDK and telemetry configuration as applications. */
export async function main(args = process.argv.slice(2), options = {}) {
  const output = options.output ?? (value => console.log(typeof value === 'string' ? value : JSON.stringify(value)));
  const program = new Command().name('brain').description('Manage Brain sessions and tools')
    .option('--url <url>', 'Brain service URL', process.env.BRAIN_URL ?? 'http://127.0.0.1:8080')
    .addHelpText('after', '\nSet BRAIN_API_KEY and optionally BRAIN_URL.')
    .configureOutput({ writeOut: text => output(text.trimEnd()), writeErr: () => {} })
    .exitOverride();
  let client;
  program.hook('preAction', () => {
    if (!options.client && !process.env.BRAIN_API_KEY) throw new Error('BRAIN_API_KEY is required');
    client = options.client ?? new Brain(program.opts().url, process.env.BRAIN_API_KEY);
  });
  program.command('create').description('Create a session')
    .requiredOption('--id <id>', 'Session ID').requiredOption('--model <model>', 'Provider/model')
    .option('--credential-env <name>', 'Environment variable containing the model credential')
    .action(async values => {
      const key = values.credentialEnv && process.env[values.credentialEnv];
      if (values.credentialEnv && !key) throw new Error('credential environment is empty');
      output(await client.create({ id: values.id, harness: Pi(), model: Model(values.model, key ? ApiKey(key) : undefined) }));
    });
  program.command('list').description('List sessions').action(async () => output(await client.list()));
  for (const name of ['get', 'history', 'interrupt', 'resume', 'close', 'delete', 'flush', 'sync']) {
    program.command(`${name} <id>`).description(`${name[0].toUpperCase() + name.slice(1)} a session`)
      .action(async id => output((await client[name](id)) ?? { status: 'ok' }));
  }
  program.command('send <id> <message...>').description('Send a message')
    .action(async (id, message) => output(await client.send(id, { type: 'user.message', text: message.join(' ') })));
  program.command('observe <id>').description('Stream session events')
    .action(async id => { for await (const event of client.observe(id)) output(event); });
  program.command('attach <id>').description('Upload an attachment')
    .requiredOption('--file <path>', 'File to upload').option('--content-type <type>', 'Media type', 'application/octet-stream')
    .action(async (id, values) => {
      const { openAsBlob } = await import('node:fs'); const { basename } = await import('node:path');
      output(await client.uploadAttachment(id, basename(values.file), await openAsBlob(values.file, { type: values.contentType })));
    });
  program.command('upload-tool').description('Upload a hosted tool package')
    .requiredOption('--file <path>', 'JSON tool package')
    .action(async values => { const pkg = JSON.parse(await readFile(values.file, 'utf8')); output(await client.uploadTool(pkg.tool, pkg.module)); });
  try {
    await program.parseAsync(args.length ? args : ['--help'], { from: 'user' });
  } catch (error) {
    if (error.code !== 'commander.helpDisplayed') throw error;
  } finally { await client?.dispose(); }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main().catch(error => { console.error(error.message); process.exitCode = 1; });
