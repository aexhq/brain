import contract from '../public/openapi.json';

// MDX renders API pages as components, so its processed Markdown omits the
// reference. Export the matching operation with its schema dependencies instead.
export function apiMarkdown(id: string): string {
  for (const [path, methods] of Object.entries(contract.paths)) {
    for (const [method, operation] of Object.entries(methods)) {
      if (operation.operationId !== id) continue;
      const schemas: Record<string, unknown> = {};
      const allSchemas: Record<string, unknown> = contract.components.schemas;
      function collect(value: unknown) {
        if (!value || typeof value !== 'object') return;
        for (const [key, item] of Object.entries(value)) {
          if (key === '$ref' && typeof item === 'string' && item.startsWith('#/components/schemas/')) {
            const name = item.slice('#/components/schemas/'.length);
            if (!(name in schemas)) { schemas[name] = allSchemas[name]; collect(schemas[name]); }
          } else collect(item);
        }
      }
      collect(operation);
      const document = {
        openapi: contract.openapi, info: contract.info, servers: contract.servers,
        paths: { [path]: { [method]: operation } },
        components: { schemas, securitySchemes: contract.components.securitySchemes },
      };
      return `${operation.description}\n\n## ${method.toUpperCase()} ${path}\n\nBearer authentication is required.\n\n\`\`\`json\n${JSON.stringify(document, null, 2)}\n\`\`\`\n\n[Complete OpenAPI document](/openapi.json)`;
    }
  }
  throw new Error(`Missing OpenAPI operation: ${id}`);
}
