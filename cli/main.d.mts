import type { Brain } from './index.js';
export { configureTelemetry, telemetry } from './index.js';
export function main(args?: string[], options?: {
  client?: Brain;
  output?: (value: unknown) => void;
}): Promise<void>;
