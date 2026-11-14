import { copyFile, chmod } from 'node:fs/promises';
const dest = new URL('../sdks/typescript/dist/cli.mjs', import.meta.url);
await copyFile(new URL('../cli/main.mjs', import.meta.url), dest);
await copyFile(new URL('../cli/main.d.mts', import.meta.url), new URL('../sdks/typescript/dist/cli.d.mts', import.meta.url));
await chmod(dest, 0o755);
