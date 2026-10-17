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
  ['docs/roadmap.md', 'roadmap.mdx', 'Roadmap', 'What comes next for Brain, and how mature each part is today.'],
]) {
  const text = (await readFile(path.join(root, from), 'utf8')).replace(/^# .*\r?\n/, '');
  await page(to, title, description, text);
}

const models = JSON.parse(execFileSync(process.execPath, ['runners/pi/scripts/model-catalog.mjs'], { cwd: root, encoding: 'utf8' }));
const runner = JSON.parse(await readFile(path.join(root, 'runners/pi/package.json'), 'utf8'));
let inventory = `Extracted from the pinned Pi \`${runner.dependencies['@earendil-works/pi-coding-agent']}\` model registry (\`runners/pi/scripts/model-catalog.mjs\`).\n\n> **Note:** This list is an offline catalog inventory, **not live-provider qualification**. Brain authenticates each provider with the API key configured in \`providers\`. See [Models & providers](/docs/reference/models) for provider configuration, reasoning effort, and custom endpoints.\n\n`;
for (const provider of [...new Set(models.map(m => m.provider))]) {
  inventory += `## ${provider}\n\n| Model ID | Transport | Context window |\n| --- | --- | ---: |\n`;
  for (const m of models.filter(m => m.provider === provider)) inventory += `| \`${m.provider}/${m.id}\` | \`${m.api}\` | ${m.contextWindow} |\n`;
  inventory += '\n';
}
await page('reference/model-catalog.mdx', 'Model catalog', 'Offline model IDs, transports, and context windows bundled in the pinned Pi runtime registry.', inventory);

let interfaces = 'Extracted directly from Go source during the documentation build. Application code imports public aliases from `github.com/aexhq/brain` for harnesses, providers, handlers, and session contracts. The public `Server` owns stock persistence and transport implementations. Repository and checkpoint adapter seams remain internal. See the [Go application SDK](/docs/sdks/application).\n\n';
for (const [pkg, symbols] of [
  ['harness', ['Factory', 'Runner', 'Options', 'RunnerFrame']],
  ['persistence', ['Objects', 'Metadata']],
  ['session', ['Repository', 'ToolExecutor', 'Checkpoints']],
  ['store', ['Metadata']],
  ['tools', ['Transport', 'Binding', 'Request']],
]) {
  interfaces += `## ${pkg}\n\n`;
  for (const symbol of symbols) {
    const code = execFileSync('go', ['doc', '-src', `./internal/${pkg}`, symbol], { cwd: root, encoding: 'utf8' });
    interfaces += `### ${symbol}\n\n\`\`\`go\n${code.trim()}\n\`\`\`\n\n`;
  }
}
await page('adapters/interfaces.mdx', 'Go interfaces', 'Session, tool, database, harness, and checkpoint contracts generated from Go source.', interfaces);
console.log(`Generated ${operations.length} API pages, ${models.length} model entries, interfaces, and shared guides.`);
