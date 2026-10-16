import { spawnSync } from "node:child_process";
import path from "node:path";

const python = path.resolve("bin/sdk-python", process.platform === "win32" ? "Scripts/python.exe" : "bin/python");

const commands = [
  ["npm", ["--prefix", "runners/pi", "run", "build"]],
  ["npm", ["--prefix", "sdks/typescript", "run", "build"]],
  ["go", ["run", "./cmd/generate", "-check"]],
  ["node", ["scripts/package-sdks.mjs"]],
  ["go", ["test", "-count=1", "./..."]],
  ["go", ["vet", "./..."]],
  ["npm", ["--prefix", "runners/pi", "test"]],
  ["npm", ["--prefix", "sdks/typescript", "test"]],
  ["npm", ["--prefix", "services/sandbox", "test"]],
  [python, ["-I", "-m", "unittest", "discover", "-s", "bin/sdk-consumer/python", "-p", "test_*.py"]],
  ["node", ["--test", "tests/journeys/journeys.test.mjs"]],
  ["node", ["--test", "tests/platforms.test.mjs"]],
  ["node", ["--test", "tests/registry-artifacts.test.mjs"]],
  ["npm", ["--prefix", "website", "run", "build"]],
  ["npm", ["--prefix", "website", "test"]],
];
for (const [command, args] of commands) {
  console.log(`\n${command} ${args.join(" ")}`);
  const result = spawnSync(command, args, {
    stdio: "inherit", shell: process.platform === "win32" && (command === "npm" || command === "python"),
    env: { ...process.env, PYTHONPATH: "" },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

