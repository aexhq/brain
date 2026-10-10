import assert from "node:assert/strict";
import test from "node:test";
import { HttpTransport } from "../dist/http.js";

test("SSE decoding preserves split UTF-8, multiline JSON and independent event cursors", async () => {
  const data = new TextEncoder().encode('id: 7\nevent: saved\ndata: {"text":\ndata: "héllo"}\n\nevent: delta\ndata: {"text":"next"}\n\n');
  let offset = 0;
  const transport = new HttpTransport({ baseUrl: "https://brain.example", token: "fixture", fetch: async (_, init) => {
    assert.equal(new Headers(init.headers).get("authorization"), "Bearer fixture");
    return new Response(new ReadableStream({
      pull(controller) { if (offset === data.length) controller.close(); else controller.enqueue(data.slice(offset, ++offset)); },
    }));
  } });
  const events = [];
  for await (const event of transport.stream("/events")) events.push(event);
  assert.deepEqual(events, [{ sequence: 7, type: "saved", data: { text: "héllo" } }, { type: "delta", data: { text: "next" } }]);
  await transport.close();
  await assert.rejects(transport.request("GET", "/events"), { name: "AbortError" });
});
