import { environmentProtocol, serveEnvironment, callExecution, createAgentloopContext } from "@aexhq/brain/runtime";
import { pathToFileURL } from "node:url";

const accepted = () => ({ type: "accepted" });
const failure = (code, message) => ({ type: "failure", code, message, retryable: false });

// This provider resolves an opaque descriptor to its JavaScript Agentloop.
// Invocation-scoped grants open only the services supplied by Brain.
export function loopEnvironment({ fetch = globalThis.fetch } = {}) {
  const sessions = new Set();
  async function turn(callback, input) {
    const ctx = createAgentloopContext(input, (method, body) => callExecution(callback, method, body, { fetch }));
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
  const handle = environmentProtocol(async op => {
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
    return receipt;
  });
  return { handle };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const env = loopEnvironment();
  const token = process.env.REFERENCE_ENV_TOKEN;
  await serveEnvironment(env.handle, { token, port: Number(process.env.PORT ?? 8091) });
}
