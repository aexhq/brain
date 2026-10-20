import assert from "node:assert/strict";
import test from "node:test";
import http from "node:http";
import { loadImages } from "../../common/media.mjs";

async function server(t, handler) {
  const instance = http.createServer(handler);
  await new Promise(resolve => instance.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise(resolve => { instance.closeAllConnections(); instance.close(resolve); }));
  return `http://127.0.0.1:${instance.address().port}`;
}

test("native media fetching enforces approved origins, redirect denial and formats", async t => {
  let forbidden = 0;
  const denied = await server(t, (_req, res) => { forbidden++; res.end(); });
  const origin = await server(t, (req, res) => {
    if (req.url === "/redirect") { res.writeHead(302, { location: denied }); res.end(); }
    else { res.writeHead(200, { "content-type": "text/html" }); res.end("not an image"); }
  });
  await assert.rejects(loadImages([{ type: "image", url: denied }], [origin]), /origin denied/);
  await assert.rejects(loadImages([{ type: "image", url: origin+"/redirect" }], [origin]));
  await assert.rejects(loadImages([{ type: "image", url: origin }], [origin]), /format/);
  assert.equal(forbidden, 0);
});

test("native image bytes have one aggregate bound across attachments", async t => {
  let requests = 0;
  const origin = await server(t, (_req, res) => { requests++; res.writeHead(200, { "content-type": "image/png" }); res.end(Buffer.alloc(11<<20)); });
  await assert.rejects(loadImages([{ type: "image", url: origin }, { type: "image", url: origin }], [origin]), /20 MiB/);
  assert.equal(requests, 2);
});

test("interrupting a pending image read cancels it without another request", async t => {
  let requests = 0;
  const started = Promise.withResolvers();
  const origin = await server(t, (_req, res) => { requests++; res.writeHead(200, { "content-type": "image/png" }); res.write("pending"); started.resolve(); });
  const controller = new AbortController();
  const pending = loadImages([{ type: "image", url: origin }], [origin], controller.signal);
  const rejected = assert.rejects(pending, { name: "AbortError" });
  await started.promise; controller.abort(); await rejected;
  assert.equal(requests, 1);
});
