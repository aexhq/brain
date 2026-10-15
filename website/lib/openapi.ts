import { createOpenAPI } from 'fumadocs-openapi/server';

export const openapi = createOpenAPI({ input: { brain: '../api/openapi.json' } });
