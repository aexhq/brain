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
  ['docs/running-persistence.md', 'guides/persistence.mdx', 'Persistence & recovery', 'Keep local execution fast. Restore from a complete external checkpoint.'],
  ['examples/tools/README.md', 'tools/local-tools.mdx', 'Local tools & approvals', 'Run echo, session inspection, and operator approval in a separate service.'],
]) {
  const text = (await readFile(path.join(root, from), 'utf8')).replace(/^# .*\r?\n/, '');
  await page(to, title, description, text);
}

const models = JSON.parse(execFileSync(process.execPath, ['runners/pi/scripts/model-catalog.mjs'], { cwd: root, encoding: 'utf8' }));
const runner = JSON.parse(await readFile(path.join(root, 'runners/pi/package.json'), 'utf8'));
let inventory = `Generated from the pinned Pi ${runner.dependencies['@earendil-works/pi-coding-agent']} runtime with network refresh and local overrides disabled.\n\nThese are **catalog entries, not live-provider qualification**. Brain currently supplies API-key credentials only. OAuth and ambient-credential providers in this catalog may require integration that Brain does not yet implement. See [model configuration](/docs/reference/models).\n\n`;
for (const provider of [...new Set(models.map(m => m.provider))]) {
  inventory += `## ${provider}\n\n| Model ID | Transport | Context window |\n| --- | --- | ---: |\n`;
  for (const m of models.filter(m => m.provider === provider)) inventory += `| \`${m.provider}/${m.id}\` | \`${m.api}\` | ${m.contextWindow} |\n`;
  inventory += '\n';
}
await page('reference/model-catalog.mdx', 'Model catalog', 'The offline model inventory bundled with Brain’s pinned Pi runtime.', inventory);

let interfaces = 'These declarations are extracted from the current Go source during the documentation build. The packages are internal to the Brain repository; implementing an adapter currently means contributing to Brain.\n\n';
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
await page('adapters/interfaces.mdx', 'Go interfaces', 'Harness and persistence extension points, generated from source.', interfaces);
console.log(`Generated ${operations.length} API pages, ${models.length} model entries, interfaces, and shared guides.`);
