export interface PiPluginOptions {
  /** Upstream package configuration, installed privately by the server. */
  config?: Record<string, unknown>;
  /** Expose only these registered tools; omitted selects the package's tools. */
  tools?: string[];
}

export interface PiPlugin extends PiPluginOptions {
  name: 'web-access' | 'subagents' | 'mcp';
}

export interface HarnessOptions {
  /** A server-local profile name. Defaults to the harness kind. */
  name?: string;
  maxShared?: number;
  maxActiveTurns?: number;
  maxHosts?: number;
  maxStarting?: number;
}

export interface PiOptions extends HarnessOptions {
  plugins?: PiPlugin[];
}

export interface Harness extends HarnessOptions {
  kind: 'pi' | 'codex' | 'opencode';
  name: string;
  plugins?: PiPlugin[];
}

function harness(kind: Harness['kind'], options: PiOptions = {}): Harness {
  const allowed = ['name', 'maxShared', 'maxActiveTurns', 'maxHosts', 'maxStarting', ...(kind === 'pi' ? ['plugins'] : [])];
  for (const key of Object.keys(options)) if (!allowed.includes(key)) throw new Error(`Unknown ${kind} option: ${key}`);
  const name = options.name ?? kind;
  if (!/^[a-zA-Z][a-zA-Z0-9_-]{0,63}$/.test(name)) throw new Error('Invalid harness name');
  for (const key of ['maxShared', 'maxActiveTurns', 'maxHosts', 'maxStarting'] as const) {
    const value = options[key];
    if (value !== undefined && (!Number.isInteger(value) || value < 1)) throw new Error(`Invalid harness ${key}`);
  }
  if (options.plugins !== undefined && (!Array.isArray(options.plugins) || options.plugins.some(p => !['web-access', 'subagents', 'mcp'].includes(p?.name)))) throw new Error('Invalid Pi plugins');
  return { ...options, kind, name };
}

export const Pi = (options: PiOptions = {}): Harness => harness('pi', options);
export const Codex = (options: HarnessOptions = {}): Harness => harness('codex', options);
export const OpenCode = (options: HarnessOptions = {}): Harness => harness('opencode', options);

function plugin(name: PiPlugin['name'], options: PiPluginOptions): PiPlugin {
  for (const key of Object.keys(options)) if (!['config', 'tools'].includes(key)) throw new Error(`Unknown Pi ${name} option: ${key}`);
  if (options.config !== undefined && (!options.config || typeof options.config !== 'object' || Array.isArray(options.config))) throw new Error('Plugin config must be an object');
  if (options.tools !== undefined && (!Array.isArray(options.tools) || options.tools.some(t => typeof t !== 'string' || !t))) throw new Error('Plugin tools must be a list of names');
  return { name, ...options };
}

export const PiWebAccess = (options: PiPluginOptions = {}): PiPlugin => plugin('web-access', options);
export const PiSubagents = (options: PiPluginOptions = {}): PiPlugin => plugin('subagents', options);
export const PiMcp = (options: PiPluginOptions = {}): PiPlugin => plugin('mcp', options);
