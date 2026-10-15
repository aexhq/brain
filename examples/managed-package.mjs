import { SessionClient } from "@aexhq/brain/v2";

const language = process.argv[2] ?? "node";
if (!["node", "python"].includes(language)) throw new Error("Choose node or python");
const client = new SessionClient({ url: process.env.BRAIN_URL, token: process.env.BRAIN_API_TOKEN });
const action = { name: "greet", description: "Return a greeting", connection: "worker",
  input_schema: { type: "object", required: ["name"], properties: { name: { type: "string" } }, additionalProperties: false },
  max_output_bytes: 8192, timeout_ms: 15000 };
const configuration = (actions, capabilities) => ({
  model: { provider: process.env.BRAIN_MODEL_PROVIDER ?? "openai", name: process.env.BRAIN_MODEL ?? "gpt-4.1-mini" },
  harness: { name: process.env.BRAIN_HARNESS_NAME ?? "portable", version: process.env.BRAIN_HARNESS_VERSION ?? "1.0.0" },
  actions, connections: [{ name: "worker", kind: "local", provider: "local_compute", ownership: "managed", capabilities }],
  events: [], grants: ["session:read", "input:write"],
  limits: { calls: 20, concurrent: 2, input_bytes: 131072, output_bytes: 65536, retained_bytes: 16777216, pending_inputs: 32 },
});
const manifest = {
  name: "managed-greeting", version: "1.0.0", language, actions: [action], secrets: [], network: [],
  memory_bytes: 134217728, cpu_millis: 1000, wall_ms: 15000,
  ...(language === "node" ? {
    entrypoint: "greet.ts",
    files: { "greet.ts": 'export default (args: { name: string }) => ({ greeting: `Hello, ${args.name}!` });',
      "package.json": JSON.stringify({ name: "managed-greeting", version: "1.0.0", type: "module" }) },
    lockfile: JSON.stringify({ name: "managed-greeting", version: "1.0.0", lockfileVersion: 3, requires: true,
      packages: { "": { name: "managed-greeting", version: "1.0.0" } } }),
  } : {
    entrypoint: "greet.py", files: { "greet.py": 'def main(args, context):\n    return {"greeting": f"Hello, {args[\'name\']}!"}\n' },
    lockfile: "",
  }),
};
async function wait(get, complete) {
  const deadline = Date.now() + 240000;
  while (Date.now() < deadline) {
    const value = await get();
    if (complete(value)) return value;
    await new Promise(resolve => setTimeout(resolve, 250));
  }
  throw new Error("Work remains unsettled; inspect its existing operation instead of resubmitting it");
}
const builder = await client.create(configuration([{ name: "build_package", description: "Build package", connection: "worker",
  input_schema: { type: "object", required: ["package_id"], properties: { package_id: { type: "string" } } },
  max_output_bytes: 8192, timeout_ms: 300000 }], ["build"]));
let runtime;
try {
  await builder.attach("worker");
  const admitted = await client.admitPackage({ request_id: crypto.randomUUID(), session_id: builder.id, builder: "worker", manifest });
  const pkg = await wait(() => client.package(admitted.id), value => {
    if (value.status === "failed" || value.status === "unknown") throw new Error(value.error ?? value.status);
    return value.status === "ready";
  });
  runtime = await client.create(configuration([{ ...action, package: pkg.id }], ["invoke"]));
  await runtime.attach("worker");
  await runtime.invoke("greet", { name: "Ada" });
  const operations = await wait(() => runtime.operations(), values => {
    if (values[0]?.status === "failed" || values[0]?.status === "unknown") throw new Error(values[0].error ?? values[0].status);
    return values[0]?.status === "completed";
  });
  console.log(operations[0].result);
} finally {
  if (runtime) await runtime.control("close");
  await builder.control("close");
}
