// The Mastra server this benchmark measures.
//
// One agent at Mastra's defaults, its model pointed at the scripted provider through
// Mastra's own model router (`custom/<name>` with an explicit base URL), so the calls
// land on the fixture's /chat/completions. Storage is explicit because Mastra ships no
// default persistent store: a LibSQL file under the run's data directory, which is what
// the persistence probe measures. MASTRA_TELEMETRY_DISABLED=1 travels in the launch
// environment.
import { Mastra } from '@mastra/core/mastra';
import { Agent } from '@mastra/core/agent';
import { Memory } from '@mastra/memory';
import { LibSQLStore } from '@mastra/libsql';

const benchAgent = new Agent({
  id: 'bench-agent',
  name: 'bench-agent',
  instructions: 'You are a benchmark assistant.',
  model: {
    id: 'custom/gpt-4o-mini',
    url: process.env.BENCH_MODEL_BASE_URL!,
    apiKey: 'bench',
  },
  memory: new Memory({ options: { lastMessages: 20 } }),
});

export const mastra = new Mastra({
  agents: { benchAgent },
  storage: new LibSQLStore({
    id: 'bench-storage',
    url: `file:${process.env.BENCH_DATA_DIR}/mastra.db`,
  }),
  server: {
    port: Number(process.env.BENCH_PORT),
    host: '127.0.0.1',
  },
});
