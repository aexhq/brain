/// <reference lib="esnext.disposable" />
import { Brain } from './sdk/index.js';
export * from './sdk/index.js';

export interface BrainServerOptions {
  /** Directory for persistent session data. Default: .brain in the working directory. */
  dataDir?: string;
  /** Loopback TCP port. Default: 0, which lets the OS choose an available port. */
  port?: number;
  /** Defaults to OpenAI using OPENAI_API_KEY. Provider keys stay in the child environment. */
  providers?: Record<string, { apiKey: string; baseUrl?: string; api?: string }>;
}

export class BrainServer extends Brain {
  private constructor();
  static start(options?: BrainServerOptions): Promise<BrainServer>;
  /** Resolves when the server exits, including an unexpected exit. */
  readonly exited: Promise<{ code: number | null; signal: string | null; error?: Error }>;
  stop(): Promise<void>;
  [Symbol.asyncDispose](): Promise<void>;
}
