import { copyFile, chmod } from 'node:fs/promises';
const dest = new URL('../sdks/typescript/dist/cli.mjs', import.meta.url);
await copyFile(new URL('../cli/main.mjs', import.meta.url), dest);
await chmod(dest, 0o755);
