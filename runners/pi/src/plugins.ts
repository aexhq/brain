import { mkdir, writeFile, readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import path from 'node:path';
import { createJiti } from 'jiti';
import type { ExtensionAPI, ExtensionFactory, ToolDefinition } from '@earendil-works/pi-coding-agent';
import type { PiPlugin, Tool } from './generated.js';

const require = createRequire(import.meta.url);
const jiti = createJiti(import.meta.url, { fsCache: false });

/** Plugins use process-global configuration, so their factory owns one session per host. */
export async function plugins(
  selected: PiPlugin[],
  call: (name: string, id: string, args: unknown, signal?: AbortSignal) => Promise<{ content: any[]; structured_content?: unknown; is_error?: boolean }>,
  progress: (id: string, value: unknown) => void,
  applicationTools: string[],
) {
  const tools: Tool[] = [];
  const pending = new Map<string, () => Promise<unknown>>();
  const closes: (() => Promise<void>)[] = [];
  const factories: ExtensionFactory[] = [];
  let childFactory: { setChildSessionFactory(factory: unknown): void } | undefined;
  if (selected.length) {
    const directory = path.join(process.cwd(), 'plugin-config');
    await mkdir(directory, { recursive: true });
    process.env.PI_CODING_AGENT_DIR = directory;
    process.env.PI_MCP_CONFIG_MODE = 'exclusive';
    for (const plugin of selected) {
      const config = plugin.config ?? {};
      let factory: ExtensionFactory;
      if (plugin.name === 'mcp') {
        const { createHostManagedMcpAdapter } = await jiti.import<any>('pi-mcp-adapter/host-managed');
        const { StreamableHTTPClientTransport } = await jiti.import<any>('@modelcontextprotocol/client');
        const { StdioClientTransport } = await jiti.import<any>('@modelcontextprotocol/client/stdio');
        const servers = Object.fromEntries(Object.entries((config as any).mcpServers ?? {}).map(([name, server]: [string, any]) => [name, {
          tools: server.tools,
          createTransport: () => server.url
            ? new StreamableHTTPClientTransport(new URL(server.url), { requestInit: { headers: server.headers }, reconnectionOptions: { maxRetries: 0 } })
            : new StdioClientTransport({ command: server.command, args: server.args, env: server.env, stderr: 'pipe' }),
        }]));
        const adapter = createHostManagedMcpAdapter({ servers, onToolCall: (invocation: any) => invocation.dispatch(), requestTimeoutMs: (config as any).requestTimeoutMs });
        closes.push(() => adapter.close());
        await adapter.ready();
        factory = pi => {
          pi.events.on('pi-mcp-adapter:tool-approval-request', (request: any) => request.claim(() => 'allow_once'));
          adapter.extensionFactory(pi);
        };
      } else {
        const settings: Record<string, any> = plugin.name === 'web-access'
          ? { workflow: 'raw', autoOpenBrowser: false, toolActivation: 'eager', fetch: { defaultMode: 'readable', allowedModes: ['readable', 'raw'] }, ...config }
          : { asyncByDefault: false, forceTopLevelAsync: false, scheduledRuns: { enabled: false }, toolActivation: 'eager', enableSlashCommands: false, ...config };
        if (plugin.name === 'web-access' && (settings.workflow !== 'raw' || settings.autoOpenBrowser !== false)) throw new Error('Brain web access requires raw workflow and no automatic browser');
        if (plugin.name === 'web-access' && (!settings.fetch?.allowedModes || settings.fetch.allowedModes.includes('answer'))) throw new Error('Brain web fetch supports readable and raw modes');
        if (plugin.name === 'subagents') {
          if (settings.asyncByDefault !== false || settings.forceTopLevelAsync !== false || settings.scheduledRuns?.enabled !== false || settings.forkContext?.mode === 'pruned') throw new Error('Brain subagents must finish within the active turn');
          settings.disabledFeatures = [...new Set([...(settings.disabledFeatures ?? []), 'workflow-scripts', 'agent-management', 'panes', 'external-machines', 'extension-bindings', 'watchdog'])];
        }
        const file = plugin.name === 'web-access' ? 'web-search.json' : 'extensions/subagent/config.json';
        await mkdir(path.dirname(path.join(directory, file)), { recursive: true });
        await writeFile(path.join(directory, file), JSON.stringify(settings), { mode: 0o600 });
        const packageName = plugin.name === 'web-access' ? 'pi-web-access' : 'pi-subagents';
        const packageRoot = plugin.name === 'web-access'
          ? path.dirname(require.resolve('pi-web-access/package.json'))
          : path.dirname(require.resolve('pi-subagents'));
        const manifest = JSON.parse(await readFile(path.join(packageRoot, 'package.json'), 'utf8'));
        const entry = plugin.name === 'web-access' ? path.join(packageRoot, manifest.pi.extensions[0], 'index.js') : path.join(packageRoot, 'index.js');
        factory = (await import(pathToFileURL(entry).href)).default;
        if (plugin.name === 'subagents') childFactory = await import(pathToFileURL(path.join(packageRoot, 'src/runs/shared/child-session.js')).href);
      }
      factories.push(async pi => {
        if (plugin.name === 'subagents') {
          const { registerSubagentCapabilityCeiling } = await import('pi-subagents/capability-ceiling');
          pi.on('session_start', (_event, ctx) => {
            const handle = registerSubagentCapabilityCeiling({ sessionId: ctx.sessionManager.getSessionId(), source: 'brain', ceiling: { allowedTools: applicationTools, allowedAgents: Object.keys((config as any).agents ?? { brain_worker: {} }), denyExtensions: true } });
            closes.push(async () => handle.dispose());
          });
        }
        const registered = new Set<string>();
        const api = new Proxy(pi, {
          get(target, key, receiver) {
            if (key !== 'registerTool') return Reflect.get(target, key, receiver);
            return (tool: ToolDefinition) => {
              if (plugin.name === 'subagents' && tool.name !== 'subagent') return;
              if (plugin.tools && !plugin.tools.includes(tool.name)) return;
              if (tools.some(t => t.name === tool.name)) throw new Error(`Duplicate plugin tool: ${tool.name}`);
              registered.add(tool.name);
              tools.push({ name: tool.name, description: tool.description, input_schema: tool.parameters });
              target.registerTool({ ...tool, exposure: 'direct', defaultActive: true,
                async execute(id, args, signal, onUpdate, ctx) {
                  // Background work would outlive Brain's turn/checkpoint boundary.
                  const parameters = args as Record<string, any>;
                  if (pending.has(id)) throw new Error('Duplicate plugin call');
                  pending.set(id, async () => {
                    try {
                      if (plugin.name === 'subagents' && (parameters.async === true || parameters.options?.async === true)) throw new Error('Background subagents are not supported by Brain');
                      const result = await tool.execute(id, args, signal, value => { onUpdate?.(value); progress(id, value); }, ctx);
                      const converted = { content: result.content.map((c: any) => c.type === 'image' ? { type: 'image', data: c.data, mime_type: c.mimeType } : { type: 'text', text: c.text ?? '' }), structured_content: result.details, is_error: (result as any).isError === true };
                      if (Buffer.byteLength(JSON.stringify(converted)) > 1 << 20) return { is_error: true, content: [{ type: 'text', text: 'Plugin output exceeds the size limit' }] };
                      return converted;
                    } catch { return { is_error: true, content: [{ type: 'text', text: 'Plugin tool failed' }] }; }
                  });
                  try {
                    const result = await call(tool.name, id, args, signal);
                    if (result.is_error) throw new Error(result.content.map(c => c.text ?? '').join('\n'));
                    return { content: result.content.map(c => c.type === 'image' ? { type: 'image' as const, data: c.data, mimeType: c.mime_type } : { type: 'text' as const, text: c.text ?? '' }), details: result.structured_content ?? {} };
                  } finally { pending.delete(id); }
                },
              });
            };
          },
        });
        await factory(api as ExtensionAPI);
        if (plugin.name === 'subagents') {
          const { registerAgent } = await import('pi-subagents/agents');
          const agents = (config as any).agents ?? { brain_worker: { description: 'A focused delegated task', systemPrompt: 'Complete the delegated task and return the result.', tools: [] } };
          for (const [name, definition] of Object.entries(agents)) {
            if ((definition as any).defaultAsync) throw new Error('Background subagents are not supported by Brain');
            registerAgent({ pi: api as ExtensionAPI, name, definition: definition as any });
          }
        }
        for (const name of plugin.tools ?? []) if (!registered.has(name)) throw new Error(`Unknown ${plugin.name} tool: ${name}`);
      });
    }
  }
  return {
    factories, tools,
    setChildFactory(factory: unknown) { childFactory?.setChildSessionFactory(factory); },
    async execute(id: string) {
      const run = pending.get(id);
      if (!run) throw new Error('Plugin call is not pending');
      pending.delete(id);
      return run();
    },
    async close() { await Promise.all(closes.map(close => close())); },
  };
}
