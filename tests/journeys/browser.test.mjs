import assert from "node:assert/strict";
import { before, after, test } from "node:test";
import http from "node:http";
import { readFile, readdir } from "node:fs/promises";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";
import { build, fixture, config, message, call, history, state, tokens, until, removeTemp } from "./fixture.mjs";
import { browserGateway } from "../../examples/browser/gateway.mjs";
import { browserService, descriptor as remoteDescriptor } from "../../services/browser/src/browser.mjs";
const requireBrowser = createRequire(new URL("../../services/browser/package.json", import.meta.url));
const requireSDK = createRequire(new URL("../../bin/sdk-consumer/package.json", import.meta.url));
const { chromium } = requireBrowser("playwright");
const sdk = path.dirname(requireSDK.resolve("@aexhq/brain"));
const sdkFiles = new Map(await Promise.all((await readdir(sdk)).filter(name => name.endsWith(".js")).map(async name => [name, await readFile(path.join(sdk, name))])));
const tool = { name: "read_tab", description: "Read the user-attached page", input_schema: { type: "object", properties: { wait: { type: "boolean" } }, additionalProperties: false } };
const listen = server => new Promise(resolve => server.listen(0, "127.0.0.1", () => resolve(`http://127.0.0.1:${server.address().port}`)));
const stop = server => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); });
let binaries;
before(async () => { binaries = await build(); }, { timeout: 120000 });
after(async () => { if (binaries) await removeTemp(binaries); });

async function attached(t, { ttlMs = 300000 } = {}) {
  let browser, gateway, server;
  t.after(async () => { await browser?.close(); await gateway?.close(); if (server) await stop(server); });
  const f = await fixture(t, binaries); await f.start();
  server = http.createServer(async (req, res) => {
    if (req.url.startsWith("/sdk/")) {
      const asset = sdkFiles.get(req.url.slice(5));
      if (!asset) { res.writeHead(404).end(); return; }
      res.writeHead(200, { "content-type": "text/javascript" }); res.end(asset); return;
    }
    if (req.url === "/") {
      res.writeHead(200, { "content-type": "text/html", "cache-control": "no-store" });
      res.end(`<button id="attach">Attach this tab</button><p id="private">private content in this tab</p><script type="module">
        import { Brain, connectTab } from '/sdk/index.js';
        window.brain = new Brain(location.origin, ${JSON.stringify(pageGrant.token)});
        window.calls = 0; window.scopes = []; window.observed = [];
        const tool = { ...${JSON.stringify(tool)}, run(args, context) { window.calls++; window.scopes.push(context.scope); return args.wait ? new Promise(resolve => window.complete = resolve) : { text: document.querySelector('#private').textContent }; } };
        window.tryWithoutGesture = () => connectTab(brain, { create: { ...${JSON.stringify(config())}, runtime_tools: [tool] } });
        window.withoutGesture = await window.tryWithoutGesture().then(() => 'incorrectly attached', e => e.message);
        document.querySelector('#attach').addEventListener('click', async () => { try {
          window.attached = await connectTab(brain, window.rebind ? { session: window.rebind, tools: [tool] } : { create: { ...${JSON.stringify(config())}, runtime_tools: [tool] } });
          window.session = attached.session;
          const controller = new AbortController(); window.stopObserve = () => controller.abort();
          (async () => { try { for await (const event of brain.observe(session.id, { signal: controller.signal })) observed.push(event); } catch {} })();
        } catch (e) { window.attachError = e.message; } });
        window.ready = true;
      </script>`); return;
    }
    await gateway.handle(req, res);
  });
  const origin = await listen(server);
  gateway = browserGateway({ origin, brainURL: f.api.url, brainToken: tokens.a });
  const pageGrant = gateway.issue({ create: config(), tools: [tool], ttlMs });
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage(); await page.goto(origin); await page.waitForFunction(() => window.ready);
  return { f, gateway, page, origin, pageGrant, async attach() { await page.click("#attach"); await page.waitForFunction(() => window.session || window.attachError); assert.equal(await page.evaluate(() => window.attachError), undefined); return page.evaluate(() => window.session); } };
}

test("browser: a user explicitly attaches a tab, streams a turn and closes it during a tool call", { timeout: 90000 }, async t => {
  const a = await attached(t), { f, page } = a;
  assert.match(await page.evaluate(() => window.withoutGesture), /user gesture/);
  const replies = [];
  page.on("request", request => { if (request.url().endsWith("/responses")) replies.push({ url: request.url(), headers: request.headers(), body: request.postData() }); });
  const s = await a.attach();
  await f.api.send(s.id, message(call("runtime_read_tab"))); await state(f.api, s.id);
  assert.equal(await page.evaluate(() => window.calls), 1);
  const scope = await page.evaluate(() => window.scopes[0]); assert.equal(scope.tenant_id, "a"); assert.equal(scope.session_id, s.id);
  await page.waitForFunction(() => window.observed.some(event => event.type === "assistant.delta"));
  assert.ok((await history(f.api, s.id)).some(event => event.type === "tool.result" && JSON.stringify(event.data).includes("private content in this tab")));
  const duplicate = await page.evaluate(async reply => (await fetch(reply.url, { method: "POST", headers: { Authorization: reply.headers.authorization, "Content-Type": "application/json" }, body: reply.body })).status, replies[0]);
  assert.equal(duplicate, 409);
  for (const forbidden of ["/sessions", "/tools", `/sessions/${"0".repeat(32)}`]) {
    const status = await page.evaluate(async route => (await fetch(route, { headers: { Authorization: `Bearer ${brain.token ?? "invalid"}` } })).status, forbidden);
    assert.ok([401, 403].includes(status));
  }
  const exposed = await page.content();
  for (const secret of [tokens.a, "local-recording-provider-key"]) assert.ok(!exposed.includes(secret));
  await f.api.send(s.id, message(call("runtime_read_tab", { wait: true })));
  await page.waitForFunction(() => window.calls === 2);
  await page.close(); await state(f.api, s.id);
  const results = (await history(f.api, s.id)).filter(event => event.type === "tool.result");
  assert.ok(results.at(-1).data.result.unknown_outcome);
  assert.equal(results.length, 2, "closing the tab replayed a dispatched call");
});

test("browser: origin, composition, expiry and revocation restrict page authority", { timeout: 90000 }, async t => {
  const a = await attached(t), { f, page, gateway, origin, pageGrant } = a;
  const request = (route, options = {}) => fetch(origin + route, { ...options, headers: { Origin: origin, Authorization: `Bearer ${pageGrant.token}`, "Content-Type": "application/json", ...options.headers } });
  assert.equal((await request("/composition", { headers: { Origin: "https://untrusted.example" } })).status, 403);
  assert.equal((await request("/composition", { method: "OPTIONS" })).status, 403);
  assert.equal((await request("/sessions", { method: "POST", body: JSON.stringify({ ...config(), model_credential: { api_key: "unapproved" } }) })).status, 403);
  assert.equal((await request("/executors", { method: "POST", body: JSON.stringify({ binding: "runtime", tools: [{ ...tool, name: "unapproved" }] }) })).status, 403);
  const expiring = gateway.issue({ create: config(), ttlMs: 100 });
  await new Promise(resolve => setTimeout(resolve, 150));
  assert.equal((await request("/composition", { headers: { Authorization: `Bearer ${expiring.token}` } })).status, 401);
  const s = await a.attach();
  await f.api.send(s.id, message(call("runtime_read_tab", { wait: true }))); await page.waitForFunction(() => window.calls === 1);
  await gateway.revoke(pageGrant.token); await state(f.api, s.id);
  assert.equal((await request(`/sessions/${s.id}`)).status, 401);
  await page.evaluate(() => window.complete("late page result"));
  const replacement = gateway.issue({ session: s.id, tools: [tool] });
  await page.evaluate(({ token, session }) => { window.brain = new brain.constructor(location.origin, token); window.rebind = session; window.session = undefined; }, { token: replacement.token, session: s.id });
  await a.attach();
  await f.api.send(s.id, message(call("runtime_read_tab"))); await state(f.api, s.id);
  assert.equal(await page.evaluate(() => window.calls), 2, "rebind replayed the abandoned tool action");
  const results = (await history(f.api, s.id)).filter(event => event.type === "tool.result");
  assert.equal(results.length, 2); assert.ok(!JSON.stringify(results).includes("late page result"));
});

test("browser: a user operates an external browser and another tenant cannot reuse its context", { timeout: 90000 }, async t => {
  let service, target;
  t.after(async () => { await service?.close(); if (target) await stop(target); });
  const f = await fixture(t, binaries);
  target = http.createServer((req, res) => { res.writeHead(200, { "content-type": "text/html" }); res.end('<title>Remote fixture</title><label>Name<input id="name"></label><button onclick="document.body.dataset.value=document.querySelector(\'input\').value;document.querySelector(\'p\').textContent=document.body.dataset.value">Save</button><p>remote initial</p>'); });
  const targetURL = await listen(target);
  service = await browserService({ token: tokens.tool, origins: [targetURL] });
  const serviceURL = await listen(service.server);
  f.settings.tools.push({ ...remoteDescriptor, url: serviceURL + "/browser", token_env: "BRAIN_TOOLS_KEY", tenants: ["a", "b"] });
  await f.start(); const s = await f.api.create(config(["browser"]));
  const invoke = async args => { await f.api.send(s.id, message(call("browser", args))); await state(f.api, s.id); return (await history(f.api, s.id)).filter(event => event.type === "tool.result").at(-1).data.result; };
  assert.ok(JSON.stringify(await invoke({ action: "navigate", url: targetURL })).includes("remote initial"));
  await invoke({ action: "fill", selector: "#name", value: "saved remotely" });
  assert.ok(JSON.stringify(await invoke({ action: "click", selector: "button" })).includes("saved remotely"));
  const screenshot = await invoke({ action: "screenshot" }); assert.equal(screenshot.content[0].mime_type, "image/png");
  const other = await fetch(serviceURL + "/browser", { method: "POST", headers: { Authorization: `Bearer ${tokens.tool}`, "Content-Type": "application/json" }, body: JSON.stringify({ tenant_id: "b", session_id: s.id, call_id: "other", arguments: { action: "navigate", url: targetURL } }) });
  assert.ok(!(await other.text()).includes("saved remotely"));
  assert.ok((await invoke({ action: "navigate", url: "http://127.0.0.1:1" })).is_error);
  assert.equal((await fetch(serviceURL + "/browser", { method: "POST", body: "{}" })).status, 401);
  await invoke({ action: "close" });
  assert.ok(JSON.stringify(await invoke({ action: "navigate", url: targetURL })).includes("remote initial"));
});
