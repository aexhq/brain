import { toolServer, failure } from './services/http.mjs';
import { sandboxService } from './services/sandbox/src/service.mjs';
import { providersFromEnv } from './services/sandbox/src/providers.mjs';
import { catalog } from './services/sandbox/src/catalog.mjs';

const names = JSON.parse(process.env.BRAIN_NODE_SANDBOXES);
const token = process.env.BRAIN_NODE_TOOL_TOKEN;
const signingKey = process.env.BRAIN_NODE_SANDBOX_SIGNING_KEY;
const variables = {
  modal: { tokenId: 'MODAL_TOKEN_ID', tokenSecret: 'MODAL_TOKEN_SECRET', app: 'MODAL_APP', image: 'MODAL_IMAGE' },
  e2b: { apiKey: 'E2B_API_KEY', template: 'E2B_TEMPLATE' },
  daytona: { apiKey: 'DAYTONA_API_KEY', apiUrl: 'DAYTONA_API_URL', target: 'DAYTONA_TARGET' },
};
const server = toolServer({ token, invoke: async (path, request, signal) => {
  const { provider, parameters, credential } = request.context ?? {};
  if (!names.includes(provider) || !parameters || !credential) return failure('invalid installed sandbox binding');
  const env = {};
  for (const [field, variable] of Object.entries(variables[provider])) env[variable] = credential[field] ?? parameters[field];
  const providers = providersFromEnv(env);
  try {
    const invoke = sandboxService({ providers, signingKey });
    return await invoke(path, { ...request, arguments: path === '/create' ? { ...request.arguments, provider } : request.arguments }, signal);
  } finally { for (const value of Object.values(providers)) value.close?.(); }
} });
server.requestTimeout = 100000;
server.headersTimeout = 10000;
server.listen(0, '127.0.0.1', () => {
  const url = `http://127.0.0.1:${server.address().port}`;
  const tools = Object.fromEntries(names.map(name => [name, catalog(url, 'local', [name]).map(tool => ({
    name: tool.name.slice('sandbox_'.length), description: tool.description,
    input_schema: tool.name === 'sandbox_create' ? { type: 'object', properties: {}, additionalProperties: false } : tool.input_schema,
  }))]));
  process.stdout.write(JSON.stringify({ url, tools }) + '\n');
});
function stop() {
  server.close(() => process.exit(0));
  setTimeout(() => process.exit(1), 5000).unref();
}
process.stdin.resume();
process.stdin.on('end', stop);
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, stop);
