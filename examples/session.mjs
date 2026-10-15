import { Brain } from "../sdks/typescript/dist/index.js";

if (!process.env.BRAIN_API_KEY) throw new Error("Set BRAIN_API_KEY before running this example");
const brain = new Brain("http://127.0.0.1:8080", process.env.BRAIN_API_KEY);
const session = await brain.create({ harness: "pi", config: { model: "openai/gpt-4.1-mini", tools: [] } });
console.log(`Session: ${session.id}`);
const input = await brain.send(session.id, { type: "user.message", text: "Say hello.", key: crypto.randomUUID() });
for await (const event of brain.observe(session.id)) {
  if (event.type === "assistant.message") console.log(event.data.text);
  if (event.input_id === input.id && event.type === "session.error") throw new Error(event.data.message);
  if (event.input_id === input.id && event.type === "session.status" && event.data.status === "idle") break;
}
// Ending observation does not close the session. It remains available to resume.

