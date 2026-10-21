/// <reference lib="esnext.disposable" />
import { Brain, type ManagedSelection } from './sdk/index.js';
export * from './sdk/index.js';

export interface BrainServerOptions {
  /** Token for remote SDK clients. Default: a random token used by this instance. */
  apiKey?: string;
  /** Directory for persistent session data. Default: .brain in the working directory. */
  dataDir?: string;
  /** Loopback TCP port. Default: 0, which lets the OS choose an available port. */
  port?: number;
  /** Defaults to OpenAI using OPENAI_API_KEY. Provider keys stay in the child environment. */
  providers?: Record<string, { apiKey?: string; baseUrl?: string; api?: string }>;
  /** Installed integrations, such as [Modal({...}), AwsS3({...})]. Credentials are optional defaults. */
  tools?: ManagedSelection[];
}

export class BrainServer extends Brain {
  constructor(options?: BrainServerOptions);
  start(): Promise<this>;
  /** Resolves when the server exits, including an unexpected exit. */
  readonly exited: Promise<{ code: number | null; signal: string | null; error?: Error }>;
  stop(): Promise<void>;
  [Symbol.asyncDispose](): Promise<void>;
}
