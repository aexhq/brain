import path from 'node:path';
import { mkdir, copyFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { existsSync } from 'node:fs';
import { createAgentSession, DefaultResourceLoader, SessionManager, SettingsManager, type AgentSession, type ModelRuntime, type ToolDefinition } from '@earendil-works/pi-coding-agent';
import type { OpenOptions } from './generated.js';

/** Foreground children share this session's authorized model and explicit tool bindings. */
export function children(options: OpenOptions, runtime: ModelRuntime, customTools: ToolDefinition[], observe: (session: AgentSession) => void) {
  const live = new Set<AgentSession>();
  return {
    async create(launch: any) {
      if (launch.machine || launch.extensionPaths?.length || launch.builtinMcpTools?.length) throw new Error('Child extensions and remote placement must be installed through Brain');
      if (launch.model && launch.model !== options.config.model && launch.model !== `${options.config.model}:${options.config.reasoning_effort ?? 'off'}`) throw new Error('Child model must match the session model and reasoning effort');
      const names: string[] = launch.tools ?? [];
      if (names.some(name => !customTools.some(tool => tool.name === name))) throw new Error('Child tool is not bound to the Brain session');
      const directory = path.join(options.dir, 'children');
      await mkdir(directory, { recursive: true });
      const settings = SettingsManager.inMemory({ cacheWarming: 'off', extensions: [], skills: [], prompts: [], themes: [] });
      const resourceLoader = new DefaultResourceLoader({ cwd: path.join(options.dir, 'work'), agentDir: directory, settingsManager: settings,
        noExtensions: true, noSkills: true, noThemes: true, noPromptTemplates: true, noContextFiles: true,
        systemPrompt: launch.systemPrompt ?? '', appendSystemPrompt: launch.appendSystemPrompt ? [launch.appendSystemPrompt] : [],
        extensionFactories: (launch.hooks ?? []).map((hook: any) => hook.factory),
      });
      await resourceLoader.reload();
      if (resourceLoader.getExtensions().errors.length) throw new Error('Subagent hooks failed to load');
      const separator = options.config.model.indexOf('/');
      const model = runtime.getModel(options.config.model.slice(0, separator), options.config.model.slice(separator + 1));
      if (!model) throw new Error('Subagent model is unavailable');
      let sessionManager: SessionManager;
      if (launch.storage.kind === 'file') {
        const snapshot = path.join(directory, randomUUID() + '.jsonl');
        // Upstream uses file storage for both new children and existing forks.
        if (existsSync(launch.storage.sessionFile)) await copyFile(launch.storage.sessionFile, snapshot);
        sessionManager = SessionManager.open(snapshot, directory);
      } else sessionManager = SessionManager.create(path.join(options.dir, 'work'), directory);
      const { session, modelFallbackMessage } = await createAgentSession({ cwd: path.join(options.dir, 'work'), agentDir: directory, modelRuntime: runtime, model,
        thinkingLevel: (options.config.reasoning_effort ?? 'off') as NonNullable<Parameters<typeof createAgentSession>[0]>['thinkingLevel'],
        resourceLoader, settingsManager: settings, sessionManager, noTools: 'builtin', tools: names, customTools: customTools.filter(tool => names.includes(tool.name)),
      });
      if (modelFallbackMessage) { session.dispose(); throw new Error('Subagent model fallback is not allowed'); }
      live.add(session);
      observe(session);
      let lifecycleError: Error | undefined;
      await session.bindExtensions({ mode: 'print', onError: error => { lifecycleError = new Error(`Subagent lifecycle failed: ${error.event}`); } });
      if (lifecycleError) { session.dispose(); live.delete(session); throw lifecycleError; }
      if (JSON.stringify(session.getActiveToolNames().sort()) !== JSON.stringify([...names].sort())) {
        session.dispose(); live.delete(session); throw new Error('Subagent tool catalog differs from its bound tools');
      }
      const dispose = async () => {
        await session.abort();
        await session.extensionRunner.emit({ type: 'session_shutdown', reason: 'quit' });
        session.dispose(); live.delete(session);
      };
      return {
        subscribe: (listener: any) => session.subscribe(listener),
        prompt: (text: string) => session.prompt(text), steer: (text: string) => session.steer(text), followUp: (text: string) => session.followUp(text),
        abort: () => session.abort(), dispose,
        hasQueuedMessages: () => session.agent.hasQueuedMessages(),
        get messages() { return session.messages; },
        get sessionFile() { return sessionManager.getSessionFile(); },
        get sessionId() { return sessionManager.getSessionId(); },
        get modelId() { return model.id; },
        contextWindow: model.contextWindow,
      };
    },
    async dispose() {
      await Promise.all([...live].map(async session => { await session.abort(); await session.extensionRunner.emit({ type: 'session_shutdown', reason: 'quit' }); session.dispose(); }));
      live.clear();
    },
  };
}
