import assert from "node:assert/strict";
import { test } from "node:test";
import http from "node:http";
import net from "node:net";
import { setTimeout as delay } from "node:timers/promises";
import { browserService } from "../src/browser.mjs";
import { originProxy } from "../src/proxy.mjs";
const token = "browser-service-fixture-0000000000";
const listen = server => new Promise(resolve => server.listen(0, "127.0.0.1", () => resolve(`http://127.0.0.1:${server.address().port}`)));
const stop = server => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); });

test("browser network proxy authorizes HTTPS tunnels before opening a destination", { timeout: 10000 }, async t => {
  let accepted = 0;
  const destination = net.createServer(socket => { accepted++; socket.once("data", bytes => socket.end(bytes)); });
  await new Promise(resolve => destination.listen(0, "127.0.0.1", resolve));
  const address = `127.0.0.1:${destination.address().port}`;
  const proxy = await originProxy([`https://${address}`]);
  t.after(async () => { await proxy.close(); await new Promise(resolve => destination.close(resolve)); });
  const connect = target => new Promise((resolve, reject) => {
    const socket = net.connect(new URL(proxy.url).port, "127.0.0.1");
    let bytes = "", sent = false;
    socket.setTimeout(2000, () => socket.destroy(new Error("tunnel timed out")));
    socket.on("connect", () => socket.write(`CONNECT ${target} HTTP/1.1\r\nHost: ${target}\r\n\r\n`));
    socket.on("data", chunk => { bytes += chunk; if (!sent && bytes.includes("200 Connection Established\r\n\r\n")) { sent = true; socket.write("authorized tunnel bytes"); } });
    socket.on("error", reject); socket.on("end", () => resolve(bytes));
  });
  assert.match(await connect("forbidden.invalid:443"), /403/); assert.equal(accepted, 0);
  assert.match(await connect(address), /authorized tunnel bytes/); assert.equal(accepted, 1);
});

test("remote browser denies redirected/subresource egress, isolates sessions and expires idle contexts", { timeout: 30000 }, async t => {
  let forbiddenRequests = 0;
  const forbidden = http.createServer((_req, res) => { forbiddenRequests++; res.end("forbidden"); });
  const forbiddenURL = await listen(forbidden);
  const target = http.createServer((req, res) => {
    if (req.url === "/redirect") { res.writeHead(302, { Location: forbiddenURL }); res.end(); return; }
    res.writeHead(200, { "content-type": "text/html" });
    res.end(`<p>per-session page</p><img src="${forbiddenURL}/image"><script>localStorage.setItem('marker','present');</script>`);
  });
  const origin = await listen(target);
  const service = await browserService({ token, origins: [origin], capacity: 1, idleMs: 200 });
  const endpoint = await listen(service.server);
  t.after(async () => { await service.close(); await stop(target); await stop(forbidden); });
  const invoke = async (session, args) => (await fetch(endpoint + "/browser", { method: "POST", headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" }, body: JSON.stringify({ tenant_id: "a", session_id: session, call_id: "fixture", arguments: args }) })).json();
  assert.ok(JSON.stringify(await invoke("one", { action: "navigate", url: origin })).includes("per-session page"));
  assert.equal(forbiddenRequests, 0, "subresource bypassed URL policy");
  assert.ok((await invoke("two", { action: "navigate", url: origin })).is_error, "context capacity was ignored");
  assert.ok((await invoke("one", { action: "navigate", url: origin + "/redirect" })).is_error);
  assert.equal(forbiddenRequests, 0, "redirect bypassed URL policy");
  assert.ok((await invoke("one", { action: "navigate", url: "file:///etc/passwd" })).is_error);
  await delay(300);
  assert.ok(JSON.stringify(await invoke("two", { action: "navigate", url: origin })).includes("per-session page"));
  assert.ok((await invoke("two", { action: "read", session_id: "one" })).is_error, "model-supplied scope was accepted");
});

test("remote browser cancellation closes an in-flight context without redispatch", { timeout: 30000 }, async t => {
  let requests = 0, pending;
  const ready = Promise.withResolvers();
  const target = http.createServer((_req, res) => { requests++; pending = res; ready.resolve(); });
  const origin = await listen(target);
  const service = await browserService({ token, origins: [origin] });
  const endpoint = await listen(service.server);
  t.after(async () => { pending?.destroy(); await service.close(); await stop(target); });
  const invoke = (args, signal) => fetch(endpoint + "/browser", { method: "POST", headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" }, body: JSON.stringify({ tenant_id: "a", session_id: "one", call_id: "fixture", arguments: args }), signal });
  const controller = new AbortController();
  const active = invoke({ action: "navigate", url: origin }, controller.signal);
  await ready.promise;
  assert.ok((await (await invoke({ action: "read" })).json()).is_error);
  assert.ok((await (await invoke({ action: "read" })).json()).is_error, "a rejected peer action cleared the active reservation");
  controller.abort(); await assert.rejects(active, { name: "AbortError" });
  await delay(100);
  assert.equal(requests, 1, "cancellation replayed the browser action");
  const closed = await (await invoke({ action: "close" })).json(); assert.ok(JSON.stringify(closed).includes("closed"));
});
