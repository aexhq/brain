import { createServer } from "node:http";
import { createAgentloopContext } from "@aexhq/brain/runtime";
import { pathToFileURL } from "node:url";

const accepted = () => ({ type: "accepted" });
const failure = (code, message) => ({ type: "failure", code, message, retryable: false });
const identifier = (value) => typeof value === "string" && /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(value);

// This provider resolves an opaque descriptor to its JavaScript Agentloop.
// Invocation-scoped grants open only the services supplied by Brain.
export function loopEnvironment({ fetch = globalThis.fetch } = {}) {
  const sessions = new Set();
  const call = async (callback, path, body) => {
    if (!callback.methods.includes(path)) throw new Error(`service ${path} is not granted`);
    const response = await fetch(callback.url, {
      method: "POST",
      headers: { authorization: `Bearer ${callback.token}`, "content-type": "application/json" },
      body: JSON.stringify({ method: path, input: body }),
    });
    if (!response.ok) throw new Error(`${path} answered ${response.status}: ${await response.text()}`);
    return response.json();
  };
  async function turn(callback, input) {
    const ctx = createAgentloopContext(input, (method, body) => call(callback, method, body));
    let after = await ctx.kv.get("observed") ?? 0;
    const transcript = [...ctx.transcript];
    const observe = async (present) => {
      const before = transcript.length;
      for (;;) {
        const page = await ctx.readEvents(after);
        if (page.events.length === 0) break;
        if (present) for (const event of page.events) {
          if (["tool_result_emitted", "tool_call_ended"].includes(event.event_type)) {
            transcript.push({ role: "user", content: [{ type: "text", text: `Tool observation (data): ${JSON.stringify(event)}` }] });
          }
        }
        after = page.next_cursor;
      }
      if (transcript.length !== before) await ctx.setTranscript(transcript);
      await ctx.kv.set("observed", after);
    };
    await observe(true);
    if (!ctx.input && transcript.length === ctx.transcript.length) return {};
    const turns = (await ctx.kv.get("turns") ?? 0) + 1;
    if (ctx.input) transcript.push({ role: "user", content: [{ type: "text", text: ctx.input.message }, ...(ctx.input.media ?? [])] });
    await ctx.setTranscript(transcript);
    await ctx.emit("remote_note", { turns });
    const result = await ctx.model({ messages: transcript });
    transcript.push(result.message);
    await ctx.setTranscript(transcript);
    await ctx.kv.set("turns", turns);
    await observe(false);
    return { result: { stop_reason: result.stop_reason } };
  }
  async function handle(command) {
    const op = command?.operation;
    if (command?.contract !== "environment/v1" || !op || !Number.isSafeInteger(op.sequence) || op.sequence < 1
      || !identifier(op.session_id) || !identifier(op.environment)) {
      throw new TypeError("invalid Environment command");
    }
    const request = op.request;
    const key = `${op.session_id}/${op.environment}`;
    let receipt;
    if (request?.type === "setup") {
      if (!request.configuration || typeof request.configuration !== "object"
        || Array.isArray(request.configuration) || Object.keys(request.configuration).length !== 0) {
        receipt = failure("invalid_configuration", "this Environment accepts no options");
      } else { sessions.add(key); receipt = accepted(); }
    } else if (request?.type === "teardown") {
      sessions.delete(key);
      receipt = accepted();
    } else if (!sessions.has(key)) {
      receipt = failure("unavailable", "Environment is absent");
    } else if (request?.type === "execute" && request.implementation?.type === "reference_agentloop") {
      if (!request.callback) receipt = failure("no_callback", "a turn outside Brain needs its callback");
      else {
        try {
          receipt = { type: "result", output: await turn(request.callback, request.input) };
        } catch (error) {
          receipt = failure("agentloop_failed", String(error.message ?? error));
        }
      }
    } else if (request?.type === "detach" || request?.type === "cancel") {
      receipt = accepted();
    } else {
      receipt = failure("unsupported", "this Environment runs an Agentloop and nothing else");
    }
    return { contract: "environment/v1", sequence: op.sequence, receipt };
  }
  return { handle };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const env = loopEnvironment();
  const token = process.env.REFERENCE_ENV_TOKEN;
  createServer(async (req, res) => {
    if (token && req.headers.authorization !== `Bearer ${token}`) { res.writeHead(401).end(); return; }
    if (req.method !== "POST" || req.url !== "/v1/operations") { res.writeHead(404).end(); return; }
    try {
      const chunks = [];
      let bytes = 0;
      for await (const chunk of req) {
        bytes += chunk.length;
        if (bytes > 32 * 1024 * 1024) throw new TypeError("command exceeds 32 MiB");
        chunks.push(chunk);
      }
      const response = await env.handle(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(response));
    } catch (error) { res.writeHead(400).end(String(error.message ?? error)); }
  }).listen(Number(process.env.PORT ?? 8091), "127.0.0.1");
}
