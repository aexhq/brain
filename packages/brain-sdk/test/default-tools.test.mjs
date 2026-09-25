import assert from "node:assert/strict";
import test from "node:test";
import { z } from "zod";
import { Brain, agentloop, environment, tool } from "../dist/index.js";

test("default placement avoids explicit names, preserves closures and reattaches the original host with model services", async t => {
  let configuration;
  let commands;
  let completed = Promise.withResolvers();
  let count = 0;
  const fetch = async (url, init) => {
    const request = new Request(url, init);
    const path = new URL(request.url).pathname;
    if (path === "/v1/hosts") return Response.json({ host_id: "host_one", token: "fixture" });
    if (path.endsWith("/commands")) return new Response(new ReadableStream({ start(controller) { commands = controller; } }), { headers: { "content-type": "text/event-stream" } });
    if (path.endsWith("/model")) {
      assert.equal(request.headers.get("authorization"), "Bearer fixture");
      const body = await request.json();
      assert.equal(body.session_id, "session");
      return Response.json({ message: body.request.messages[0], stop_reason: "end_turn", usage: {} });
    }
    if (path.endsWith("/results")) { completed.resolve(await request.json()); return Response.json({ sequence: 8 }); }
    if (path.endsWith("/events")) return Response.json({ events: [{ sequence: 1, recorded_at_ms: 0, event_type: "session_creation_ended", data: { configuration } }], next_cursor: 1 });
    if (path === "/v1/sessions" && request.method === "POST") configuration = await request.json();
    return Response.json({ session_id: "session", status: "idle", last_sequence: 1 });
  };
  const echo = tool({ name: "echo", description: "Echo", input: z.object({}), run: async (_, ctx) => {
    count++;
    const response = await ctx.model({ messages: [{ role: "user", content: [{ type: "text", text: "private" }] }] });
    return ctx.finish({ count, response }, { content: "ready" });
  } });
  let client = new Brain({ baseUrl: "https://brain.example", fetch });
  t.after(() => client.close());
  const remote = environment({ url: () => "https://loop.example" })({ name: "brain-sdk-host" });
  await client.sessions.create({ environmentLifecycle: { default: "automatic" }, model: { provider: "openai", name: "test", apiKey: "fixture" }, agentloop: agentloop({ implementation: {} })({ env: remote }), tools: [echo()] });
  assert.equal(configuration.environments[1].name, "brain-sdk-host-1");
  assert.deepEqual(configuration.tools[0].placements, { "brain-sdk-host-1": { implementation: { type: "host_function", name: "echo" } } });
  const credentials = await client.credentials();
  const invoke = async sequence => {
    completed = Promise.withResolvers();
    commands.enqueue(new TextEncoder().encode(`event: command\ndata: ${JSON.stringify({ session_id: "session", environment: "brain-sdk-host-1", sequence, operation: { type: "invoke_tool", name: "echo", input: {} } })}\n\n`));
    return (await completed.promise).update.outcome;
  };
  assert.equal((await invoke(2)).value.count, 1);
  await client.close();
  client = new Brain({ baseUrl: "https://brain.example", credentials, fetch });
  await client.sessions.get("session", { tools: [echo()] });
  const second = await invoke(10);
  assert.equal(second.value.count, 2);
  assert.equal(second.content, "ready");
});
