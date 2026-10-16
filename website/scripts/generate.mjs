import { mkdir, readFile, readdir, unlink, writeFile, copyFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { generateFiles } from 'fumadocs-openapi';
import { createOpenAPI } from 'fumadocs-openapi/server';

const root = fileURLToPath(new URL('../../', import.meta.url));
const site = path.join(root, 'website');
const content = path.join(site, 'content/docs');
const apiDir = path.join(content, 'api/generated');
await mkdir(apiDir, { recursive: true });
// A removed route must disappear from both the site and the search index.
// Only this generator's flat, disposable output directory is touched.
for (const entry of await readdir(apiDir, { withFileTypes: true })) {
  if (entry.isFile() && (entry.name.endsWith('.mdx') || entry.name === 'meta.json')) await unlink(path.join(apiDir, entry.name));
}
await generateFiles({
  input: createOpenAPI({ input: { brain: path.join(root, 'api/openapi.json') } }),
  output: apiDir, includeDescription: true, addGeneratedComment: true,
});
const contract = JSON.parse(await readFile(path.join(root, 'api/openapi.json'), 'utf8'));
const operations = Object.values(contract.paths).flatMap(item => Object.values(item).map(op => op.operationId));
await writeFile(path.join(apiDir, 'meta.json'), JSON.stringify({ title: 'Endpoints', pages: operations }));
await mkdir(path.join(site, 'public'), { recursive: true });
await copyFile(path.join(root, 'api/openapi.json'), path.join(site, 'public/openapi.json'));

async function page(file, title, description, body) {
  const target = path.join(content, file);
  await mkdir(path.dirname(target), { recursive: true });
  await writeFile(target, `---\ntitle: ${JSON.stringify(title)}\ndescription: ${JSON.stringify(description)}\n---\n\n${body}\n`);
}

for (const [from, to, title, description] of [
  ['docs/running-persistence.md', 'guides/persistence.mdx', 'Persistence & recovery', 'Configure background checkpoints across filesystem, S3, R2, and GCS, and hand off sessions between servers.'],
  ['examples/tools/README.md', 'tools/local-tools.mdx', 'Local tools & approvals', 'Run the example Go tool server with HTTP JSON echo, delegated session inspection, and SSE operator approvals.'],
  ['services/sandbox/README.md', 'tools/sandboxes.mdx', 'Sandboxes', 'Create, execute, inspect, and terminate remote Modal, E2B, and Daytona sandboxes over the HTTP tool protocol.'],
  ['services/resources/README.md', 'tools/resources.mdx', 'Storage & web', 'Run tenant-scoped S3/R2 text storage, public web fetch, and Brave/Tavily web search tools.'],
  ['examples/platforms/README.md', 'guides/platforms.mdx', 'Platform examples', 'Single-owner deployment examples for Docker Compose, Fly.io, AWS ECS/Fargate, and Cloudflare R2 tool Workers.'],
]) {
  const text = (await readFile(path.join(root, from), 'utf8')).replace(/^# .*\r?\n/, '');
  await page(to, title, description, text);
}

const models = JSON.parse(execFileSync(process.execPath, ['runners/pi/scripts/model-catalog.mjs'], { cwd: root, encoding: 'utf8' }));
const runner = JSON.parse(await readFile(path.join(root, 'runners/pi/package.json'), 'utf8'));
let inventory = `Extracted from the pinned Pi \`${runner.dependencies['@earendil-works/pi-coding-agent']}\` model registry (\`runners/pi/scripts/model-catalog.mjs\`).\n\n> **Note:** This list is an offline catalog inventory, **not live-provider qualification**. Brain passes explicit API keys configured in \`providers\`; providers that require interactive OAuth or ambient host credentials are not supported out of the box. See [Models & providers](/docs/reference/models) for provider configuration, reasoning effort, and custom endpoints.\n\n`;
for (const provider of [...new Set(models.map(m => m.provider))]) {
  inventory += `## ${provider}\n\n| Model ID | Transport | Context window |\n| --- | --- | ---: |\n`;
  for (const m of models.filter(m => m.provider === provider)) inventory += `| \`${m.provider}/${m.id}\` | \`${m.api}\` | ${m.contextWindow} |\n`;
  inventory += '\n';
}
await page('reference/model-catalog.mdx', 'Model catalog', 'Offline model IDs, transports, and context windows bundled in the pinned Pi runtime registry.', inventory);

let interfaces = 'Extracted directly from the Go source (`internal/harness` and `internal/persistence`) during the documentation build. Because these packages live under `internal/`, adding a compiled Go harness or persistence adapter requires modifying the Brain codebase directly (child-process harnesses and HTTP/SSE/MCP tool services run out of process without Go changes).\n\n';
for (const [pkg, symbols] of [
  ['harness', ['Factory', 'Runner', 'Options', 'RunnerFrame']],
  ['persistence', ['Objects', 'Metadata']],
]) {
  interfaces += `## ${pkg}\n\n`;
  for (const symbol of symbols) {
    const code = execFileSync('go', ['doc', '-src', `./internal/${pkg}`, symbol], { cwd: root, encoding: 'utf8' });
    interfaces += `### ${symbol}\n\n\`\`\`go\n${code.trim()}\n\`\`\`\n\n`;
  }
}
await page('adapters/interfaces.mdx', 'Go interfaces', 'Harness and persistence Go interfaces and structs generated from internal/harness and internal/persistence.', interfaces);
console.log(`Generated ${operations.length} API pages, ${models.length} model entries, interfaces, and shared guides.`);
