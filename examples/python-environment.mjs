import { spawn } from "node:child_process";
import { resolve } from "node:path";

function command(executable, args, cwd, input) {
  return new Promise((accept, reject) => {
    const child = spawn(executable, args, { cwd, stdio: ["pipe", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.setEncoding("utf8").on("data", (chunk) => { stdout += chunk; });
    child.stderr.setEncoding("utf8").on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.stdin.on("error", reject);
    child.on("close", (code) => code === 0 ? accept(stdout) : reject(new Error(`command exited ${code}: ${stderr}`)));
    child.stdin.end(input === undefined ? undefined : JSON.stringify(input));
  });
}

// Projects are provisioned by this Environment's operator, not paths supplied by a
// Tool or a session. Run this example inside an appropriately isolated OS environment.
export function pythonEnvironment({ projects, uv = "uv" }) {
  const programs = new Map(Object.entries(projects).map(([name, project]) => [name, {
    ...project, directory: resolve(project.directory),
  }]));
  const installations = new Map();
  const sessions = new Set();
  const invoke = (project, module, input) => command(uv,
    ["run", "--no-sync", "--project", project.directory, "python", "-m", module], project.directory, input);
  async function prepare(project) {
    await command(uv, ["sync", "--locked", "--project", project.directory], project.directory);
    if (project.setup) await invoke(project, project.setup);
  }
  return {
    async handle({ contract, operation }) {
      if (contract !== "environment/v1" || !operation || !Number.isSafeInteger(operation.sequence) || operation.sequence < 1
        || ![operation.session_id, operation.environment].every((value) => typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(value))) {
        throw new TypeError("invalid Environment command");
      }
      const { request } = operation;
      const session = `${operation.session_id}/${operation.environment}`;
      let receipt;
      const failure = (code, message) => ({ type: "failure", code, message, retryable: false });
      if (request.type === "setup") {
        if (!request.configuration || typeof request.configuration !== "object" || Array.isArray(request.configuration)
          || Object.keys(request.configuration).length !== 0) {
          receipt = failure("invalid_configuration", "this Environment accepts no session options");
        } else { sessions.add(session); receipt = { type: "accepted" }; }
      } else if (request.type === "detach" || request.type === "teardown") {
        sessions.delete(session);
        receipt = { type: "accepted" };
      } else if (!sessions.has(session)) {
        receipt = failure("unavailable", "Environment is not attached");
      } else if (request.type === "execute") {
        const descriptor = request.implementation;
        const project = descriptor?.type === "python_project" && programs.get(descriptor.name);
        if (!project) receipt = failure("unsupported", "unknown Python project implementation");
        else {
          try {
            // A failed preparation stays failed until the operator replaces this
            // loader. Neither a later invocation nor a new session retries it silently.
            if (!installations.has(project.directory)) installations.set(project.directory, prepare(project));
            await installations.get(project.directory);
          } catch (error) {
            receipt = failure("preparation_failed", error.message);
          }
          if (!receipt) {
            try {
              const output = JSON.parse(await invoke(project, project.module, request.input));
              receipt = { type: "result", output };
            } catch (error) {
              receipt = failure("execution_failed", error.message);
            }
          }
        }
      } else receipt = failure("unsupported", "operation is not supported by this example");
      return { contract, sequence: operation.sequence, receipt };
    },
  };
}
