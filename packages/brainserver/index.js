import { hkdfSync, randomBytes } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { Brain, Pi, Codex, OpenCode } from './sdk/index.js';
import { startProcess, stopProcess } from './process.js';

export * from './sdk/index.js';
const require = createRequire(import.meta.url);

/** Configure an application before starting its server and installed integrations. */
export class BrainServer extends Brain {
  #environment;
  #providers = {};
  #tools;
  #harnesses;
  #dataDir;
  #port;
  #starting;
  #stopping;
  #main;
  #toolHost;
  #closingTools;
  #exited;

  constructor(options = {}) {
    checkOptions(options, ['apiKey', 'dataDir', 'port', 'providers', 'tools', 'harnesses'], 'BrainServer');
    const token = options.apiKey ?? randomBytes(32).toString('hex');
    if (typeof token !== 'string' || Buffer.byteLength(token) < 24) throw new Error('Brain apiKey must contain at least 24 bytes');
    super('', token);
    const { dataDir = '.brain', port = 0, tools = [], providers = {
      openai: process.env.OPENAI_API_KEY ? { apiKey: process.env.OPENAI_API_KEY } : {},
    } } = options;
    if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error('port must be an integer between 0 and 65535');
    if (!Array.isArray(tools)) throw new TypeError('tools must be a list of integration constructors');
    this.#dataDir = path.resolve(dataDir);
    this.#port = port;
    this.#environment = { ...process.env, BRAIN_NODE_TOKEN: token };
    const harnesses = options.harnesses ?? [Pi(), Codex(), OpenCode()];
    if (!Array.isArray(harnesses) || !harnesses.length) throw new Error('harnesses must contain at least one harness constructor');
    this.#harnesses = {};
    for (const definition of harnesses) {
      const { kind, ...settings } = definition;
      const selected = ({ pi: Pi, codex: Codex, opencode: OpenCode })[kind]?.(settings);
      if (!selected) throw new Error('Unknown harness kind');
      if (this.#harnesses[selected.name]) throw new Error(`Duplicate harness: ${selected.name}`);
      if (selected.plugins?.length && ((selected.maxShared ?? 1) !== 1 || (selected.maxActiveTurns ?? 1) !== 1)) throw new Error('Pi plugins require maxShared and maxActiveTurns of 1');
      this.#harnesses[selected.name] = {
        kind,
        max_shared: selected.plugins?.length ? 1 : selected.maxShared,
        max_active_turns: selected.plugins?.length ? 1 : selected.maxActiveTurns,
        max_hosts: selected.maxHosts, max_starting: selected.maxStarting,
        plugins: selected.plugins,
      };
    }
    for (const [name, provider] of Object.entries(providers)) {
      checkOptions(provider, ['apiKey', 'baseUrl', 'api'], `provider ${name}`);
      const { apiKey, baseUrl, api } = provider;
      if (apiKey !== undefined && (typeof apiKey !== 'string' || !apiKey)) throw new Error(`Set an API key for provider ${name}`);
      let key;
      if (apiKey !== undefined) {
        key = `BRAIN_NODE_PROVIDER_${Object.keys(this.#providers).length}`;
        this.#environment[key] = apiKey;
      }
      this.#providers[name] = { api_key_env: key, base_url: baseUrl, api };
    }
    this.#tools = tools.map((tool, index) => {
      if (!tool || tool.kind !== 'managed' || !['aws_s3', 'modal', 'e2b', 'daytona'].includes(tool.name) || tool.alias !== undefined || !Array.isArray(tool.operations)) {
        throw new Error('Server tools must use AwsS3(), Modal(), E2B(), or Daytona()');
      }
      const definition = { name: tool.name, parameters: structuredClone(tool.parameters), operations: [...tool.operations] };
      if (tool.credential !== undefined) {
        definition.credential_env = `BRAIN_NODE_TOOL_CREDENTIAL_${index}`;
        this.#environment[definition.credential_env] = JSON.stringify(tool.credential);
      }
      return definition;
    });
  }

  start() {
    if (this.#stopping) return Promise.reject(new Error('This BrainServer has been stopped; create a new instance'));
    return this.#starting ??= this.#launch();
  }

  async create(input, signal) {
    if (!('config' in input)) {
      const selected = input.harness;
      const installed = this.#harnesses[selected?.name];
      if (!installed || installed.kind !== selected.kind) throw new Error('Harness profile is not installed on this BrainServer');
      const fields = { plugins: 'plugins', maxShared: 'max_shared', maxActiveTurns: 'max_active_turns', maxHosts: 'max_hosts', maxStarting: 'max_starting' };
      for (const [key, field] of Object.entries(fields)) {
        if (selected[key] !== undefined && !isDeepStrictEqual(selected[key], installed[field])) throw new Error(`Harness ${key} differs from the installed profile`);
      }
      input = { ...input, harness: { kind: selected.kind, name: selected.name } };
    }
    return super.create(input, signal);
  }

  get exited() {
    if (!this.#exited) throw new Error('BrainServer has not started');
    return this.#exited;
  }

  async #launch() {
    const platformPackage = `@aexhq/brainserver-${process.platform}-${process.arch}`;
    let binary;
    try { binary = require.resolve(`${platformPackage}/${process.platform === 'win32' ? 'brain.exe' : 'brain'}`); }
    catch (cause) { throw new Error(`Missing ${platformPackage}. Install brainserver with optional dependencies enabled on a supported platform.`, { cause }); }
    const runtime = path.join(path.dirname(binary), 'runtime');
    for (const harness of Object.values(this.#harnesses)) harness.command = [process.execPath, harness.kind === 'codex'
      ? require.resolve(`@aexhq/brainserver-codex-${process.platform}-${process.arch}/runtime/runners/codex/src/main.mjs`)
      : path.join(runtime, 'runners', harness.kind, harness.kind === 'pi' ? 'dist/main.js' : 'src/main.mjs')];
    const directory = await mkdtemp(path.join(tmpdir(), 'brainserver-'));
    try {
      await mkdir(this.#dataDir, { recursive: true });
      const keyFile = path.join(this.#dataDir, 'credential.key');
      try { await writeFile(keyFile, randomBytes(32), { flag: 'wx', mode: 0o600 }); }
      catch (error) { if (error.code !== 'EEXIST') throw error; }
      const key = await readFile(keyFile);
      if (key.length !== 32) throw new Error('Brain credential.key must contain 32 bytes');
      this.#environment.BRAIN_NODE_CREDENTIAL_KEY = key.toString('base64');
      const sandboxes = this.#tools.filter(tool => tool.name !== 'aws_s3');
      if (sandboxes.length) {
        const token = randomBytes(32).toString('hex');
        this.#toolHost = await startProcess(process.execPath, [path.join(runtime, 'sandbox.js')], {
          ...process.env,
          BRAIN_NODE_TOOL_TOKEN: token,
          BRAIN_NODE_SANDBOXES: JSON.stringify(sandboxes.map(tool => tool.name)),
          BRAIN_NODE_SANDBOX_SIGNING_KEY: Buffer.from(hkdfSync('sha256', key, '', 'brain-sandbox-handles', 32)).toString('hex'),
        });
        this.#environment.BRAIN_NODE_TOOL_TOKEN = token;
        for (const tool of sandboxes) {
          tool.url = this.#toolHost.ready.url;
          tool.token_env = 'BRAIN_NODE_TOOL_TOKEN';
          tool.tools = this.#toolHost.ready.tools[tool.name];
        }
      }
      const config = path.join(directory, 'brain.json');
      const integrations = path.join(directory, 'integrations.json');
      await writeFile(integrations, JSON.stringify(this.#tools), { mode: 0o600 });
      await writeFile(config, JSON.stringify({
        listen: `127.0.0.1:${this.#port}`, public_url: `http://127.0.0.1:${this.#port}`, data_dir: this.#dataDir,
        tokens: [{ token_env: 'BRAIN_NODE_TOKEN', tenant: 'local', manage: true }],
        harnesses: this.#harnesses,
        providers: this.#providers, tools: [],
        credential_keys: { active: 'local', keys: { local: 'BRAIN_NODE_CREDENTIAL_KEY' } },
      }), { mode: 0o600 });
      this.#main = await startProcess(binary, ['-config', config, '-tool-providers', integrations, '-stdio'], this.#environment);
      this.url = this.#main.ready.url;
      this.#exited = this.#main.exited.then(async result => {
        try { await this.#closeTools(); return result; }
        catch (error) { return { ...result, error }; }
      });
      return this;
    } catch (error) {
      await this.#closeTools();
      throw error;
    } finally { await rm(directory, { recursive: true, force: true }); }
  }

  #closeTools() {
    return this.#closingTools ??= this.#toolHost ? stopProcess(this.#toolHost) : Promise.resolve();
  }

  stop() {
    return this.#stopping ??= (async () => {
      if (!this.#starting) return;
      await this.#starting;
      try { await this.dispose(); }
      finally {
        const result = await this.#main.stop();
        const completed = await this.exited;
        if (completed.error) throw completed.error;
        if (result.code !== 0) throw new Error(`Brain exited with ${result.code ?? result.signal}`);
      }
    })();
  }

  async [Symbol.asyncDispose]() { await this.stop(); }
}

function checkOptions(value, allowed, label) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new TypeError(`${label} options must be an object`);
  for (const key of Object.keys(value)) if (!allowed.includes(key)) throw new Error(`Unknown ${label} option: ${key}`);
}
