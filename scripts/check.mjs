import { spawnSync } from "node:child_process";

const commands = [
  ["go", ["run", "./services/brain/cmd/generate", "-check"]],
  ["node", ["sdks/typescript/scripts/generate.mjs", "--check"]],
  ["npm", ["--prefix", "runtimes/pi", "run", "build"]],
  ["npm", ["--prefix", "runtimes/codex", "run", "build"]],
  ["npm", ["--prefix", "runtimes/opencode", "run", "build"]],
  ["npm", ["--prefix", "sdks/typescript", "run", "build"]],
  ["node", ["scripts/package-sdks.mjs"]],
  ["npm", ["run", "build", ...(process.env.CI && process.platform === 'linux' ? ['--', '--all-platforms'] : [])]],
  ["node", ["--test", "tests/server-package.test.mjs"]],
  ["go", ["test", "-count=1", "./..."]],
  ["go", ["vet", "./..."]],
  ["npm", ["--prefix", "runtimes/pi", "test"]],
  ["npm", ["--prefix", "sdks/typescript", "test"]],
  ["npm", ["--prefix", "tools/sandbox", "test"]],
  ["npm", ["--prefix", "tools/browser", "test"]],
  ["node", ["--test", "tests/journeys/journeys.test.mjs", "tests/journeys/adapters.test.mjs", "tests/journeys/browser.test.mjs"]],
  ["node", ["--test", "tests/platforms.test.mjs"]],
  ["node", ["--test", "tests/registry-artifacts.test.mjs"]],
  ["npm", ["--prefix", "website", "run", "build"]],
  ["npm", ["--prefix", "website", "test"]],
];
for (const [command, args] of commands) {
  console.log(`\n${command} ${args.join(" ")}`);
  const result = spawnSync(command, args, {
    stdio: "inherit", shell: process.platform === "win32" && command === "npm",
    env: process.env,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

