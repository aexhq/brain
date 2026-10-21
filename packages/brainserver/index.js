import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { Brain } from './sdk/index.js';

export * from './sdk/index.js';

const require = createRequire(import.meta.url);

/** A local Brain server with the same session API as the remote client. */
export class BrainServer extends Brain {
  #child;
  #stop;

  constructor(url, token, child, exited) {
    super(url, token);
    this.#child = child;
    this.exited = exited;
  }

  static async start(options = {}) {
    checkOptions(options, ['dataDir', 'port', 'providers'], 'BrainServer');
    const { dataDir = '.brain', port = 0, providers = {
      openai: { apiKey: process.env.OPENAI_API_KEY },
    } } = options;
    if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error('port must be an integer between 0 and 65535');
    const env = { ...process.env, BRAIN_NODE_TOKEN: randomBytes(32).toString('hex') };
    const configured = {};
    for (const [name, provider] of Object.entries(providers)) {
      checkOptions(provider, ['apiKey', 'baseUrl', 'api'], `provider ${name}`);
      const { apiKey, baseUrl, api } = provider;
      if (typeof apiKey !== 'string' || !apiKey) throw new Error(`Set an API key for provider ${name}`);
      const key = `BRAIN_NODE_PROVIDER_${Object.keys(configured).length}`;
      env[key] = apiKey;
      configured[name] = { api_key_env: key, base_url: baseUrl, api };
    }
    const platformPackage = `@aexhq/brainserver-${process.platform}-${process.arch}`;
    let binary;
    try {
      binary = require.resolve(`${platformPackage}/${process.platform === 'win32' ? 'brain.exe' : 'brain'}`);
    } catch (cause) {
      throw new Error(`Missing ${platformPackage}. Install brainserver with optional dependencies enabled on a supported platform.`, { cause });
    }
    const directory = await mkdtemp(path.join(tmpdir(), 'brainserver-'));
    let child, exited;
    try {
      const config = path.join(directory, 'brain.json');
      await writeFile(config, JSON.stringify({
        listen: `127.0.0.1:${port}`, public_url: `http://127.0.0.1:${port}`, data_dir: path.resolve(dataDir),
        tokens: [{ token_env: 'BRAIN_NODE_TOKEN', tenant: 'local', manage: true }],
        harnesses: { pi: { command: [process.execPath, fileURLToPath(new URL('./runners/pi/dist/main.js', import.meta.url))] } },
        providers: configured, tools: [],
      }), { mode: 0o600 });
      child = spawn(binary, ['-config', config, '-stdio'], { env, windowsHide: true, stdio: ['pipe', 'pipe', 'inherit'] });
      exited = new Promise(resolve => {
        child.once('error', error => resolve({ code: null, signal: null, error }));
        child.once('exit', (code, signal) => resolve({ code, signal }));
      });
      // A rejected startup can close stdin after the child has already exited.
      child.stdin.on('error', error => { if (error.code !== 'EPIPE') child.kill(); });
      const lines = createInterface({ input: child.stdout });
      let timer;
      try {
        const ready = new Promise((resolve, reject) => {
          lines.once('line', line => { try { resolve(JSON.parse(line)); } catch (error) { reject(error); } });
          timer = setTimeout(() => reject(new Error('Brain startup timed out')), 30000);
        });
        const { url } = await Promise.race([ready, exited.then(result => { throw result.error ?? new Error(`Brain exited before startup (${result.code ?? result.signal})`); })]);
        return new BrainServer(url, env.BRAIN_NODE_TOKEN, child, exited);
      } finally {
        clearTimeout(timer);
        lines.close();
      }
    } catch (error) {
      if (child && exited) await stopChild(child, exited);
      throw error;
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  }

  /** Stop client tool executors, then drain the server and its harnesses. */
  stop() {
    return this.#stop ??= (async () => {
      try { await this.dispose(); }
      finally { await stopChild(this.#child, this.exited); }
    })();
  }

  async [Symbol.asyncDispose]() { await this.stop(); }
}

// Reject misspelled settings before they select a default directory or provider URL.
function checkOptions(value, allowed, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new TypeError(`${label} options must be an object`);
  for (const key of Object.keys(value)) {
    if (!allowed.includes(key)) throw new Error(`Unknown ${label} option: ${key}`);
  }
}

async function stopChild(child, exited) {
  child.stdin.end();
  let timer;
  try {
    const result = await Promise.race([exited, new Promise(resolve => {
      timer = setTimeout(() => { child.kill('SIGKILL'); resolve({ error: new Error('Brain shutdown timed out') }); }, 25000);
    })]);
    if (result.error) { await exited; throw result.error; }
    if (result.code !== 0) throw new Error(`Brain exited with ${result.code ?? result.signal}`);
  } finally { clearTimeout(timer); }
}
