'use client';

import { createOpenAPIPage } from 'fumadocs-openapi/ui';
import { createCodeUsageGeneratorRegistry } from 'fumadocs-openapi/requests/generators';
import { registerDefault } from 'fumadocs-openapi/requests/generators/all';

// Every documented Brain operation requires a bearer grant. Without a
// playground, Fumadocs leaves authentication out of its default examples.
const codeUsages = registerDefault(createCodeUsageGeneratorRegistry());
for (const [id, generator] of codeUsages.map()) {
  codeUsages.add(id, {
    ...generator,
    generate: (data, context) => generator.generate({
      ...data, header: { ...data.header, Authorization: { value: 'Bearer <BRAIN_TOKEN>' } },
    }, context),
  });
}

// Brain has no browser CORS policy. Show executable code samples without a
// playground that would require a credential-forwarding proxy to work.
export const OpenAPIPage = createOpenAPIPage({ codeUsages, playground: { enabled: false } });
