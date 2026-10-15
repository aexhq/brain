import { pathToFileURL } from 'node:url';

export function catalog(url, tenant, providers) {
  const object = properties => ({ type: 'object', properties, required: Object.keys(properties), additionalProperties: false });
  const sandbox = { type: 'string', description: 'The signed sandbox handle returned by sandbox_create.' };
  return [
    ['create', 'Create a short-lived Linux sandbox for this session. Python 3 is installed.', object({ provider: { type: 'string', enum: providers } })],
    ['exec', 'Execute a shell command. Returns exit code, stdout, stderr, timeout and truncation flags. Execution is limited to 30 seconds and 64 KiB output.', object({ sandbox, command: { type: 'string', maxLength: 16384 } })],
    ['inspect', 'Inspect this session sandbox without restarting it.', object({ sandbox })],
    ['terminate', 'Terminate and remove this session sandbox.', object({ sandbox })],
  ].map(([name, description, input_schema]) => ({ name: `sandbox_${name}`, description, input_schema, url: `${url}/${name}`, token_env: 'SANDBOX_TOOL_TOKEN', tenants: [tenant], timeout_seconds: 90 }));
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [url, tenant, providers] = process.argv.slice(2);
  if (!url || !tenant || !providers) throw new Error('usage: npm run catalog -- http://localhost:8091 TENANT modal,e2b,daytona');
  console.log(JSON.stringify(catalog(url, tenant, providers.split(',')), null, 2));
}
