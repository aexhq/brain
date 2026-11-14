import { createInterface } from 'node:readline';
import { inspect } from 'node:util';

// Diagnostics never share the canonical result stream.
for (const name of ['log', 'info', 'warn', 'error', 'debug']) {
  console[name] = (...values) => process.stderr.write(values.map(value => inspect(value)).join(' ') + '\n');
}
const lines = createInterface({ input: process.stdin });
const line = await new Promise((resolve, reject) => {
  lines.once('line', resolve);
  lines.once('close', () => reject(new Error('request stream closed')));
  lines.once('error', reject);
});
lines.close();
const emit = value => {
  const data = JSON.stringify(value) + '\n';
  if (Buffer.byteLength(data) > 2 * 1024 * 1024) throw new Error('tool output exceeds limit');
  return data;
};
let result;
try {
  const request = JSON.parse(line);
  const tool = (await import('/workspace/tool.mjs')).default;
  if (typeof tool !== 'function') throw new Error('module must export a default function');
  result = await tool(request, progress => process.stdout.write(emit({ progress })));
} catch (error) {
  result = { content: [{ type: 'text', text: error instanceof Error ? error.message : 'hosted tool failed' }], is_error: true };
}
try {
  process.stdout.write(emit({ result }), () => process.exit(0));
} catch {
  process.stdout.write(JSON.stringify({ result: { content: [{ type: 'text', text: 'hosted tool output exceeded limit' }], is_error: true, unknown_outcome: true } }) + '\n', () => process.exit(1));
}
