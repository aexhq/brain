import { toolServer } from '../../http.mjs';
import { sandboxService } from './service.mjs';
import { providersFromEnv } from './providers.mjs';

const invoke = sandboxService({ providers: providersFromEnv(), signingKey: process.env.SANDBOX_SIGNING_KEY });
const server = toolServer({ token: process.env.TOOL_API_KEY, invoke });
server.requestTimeout = 100000;
server.headersTimeout = 10000;
server.listen(Number(process.env.PORT ?? 8091), process.env.HOST ?? '127.0.0.1', () => console.log('sandbox tools listening'));
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => { server.close(() => process.exit(0)); setTimeout(() => process.exit(1), 5000).unref(); });
