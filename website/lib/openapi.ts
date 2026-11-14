import { createOpenAPI } from 'fumadocs-openapi/server';

export const openapi = createOpenAPI({ input: { brain: '../services/brain/api/openapi.json' } });
