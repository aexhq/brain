import { chromium } from "playwright";
import { toolServer, result, failure } from "../../http.mjs";
import { originProxy } from "./proxy.mjs";

export const descriptor = {
  name: "browser", description: "Navigate, inspect and interact with an isolated remote browser owned by this session",
  input_schema: { type: "object", properties: { action: { type: "string", enum: ["navigate", "read", "click", "fill", "screenshot", "close"] }, url: { type: "string", maxLength: 4096 }, selector: { type: "string", maxLength: 1024 }, value: { type: "string", maxLength: 32768 } }, required: ["action"], additionalProperties: false },
};

export async function browserService({ token, origins, capacity = 8, idleMs = 300000 }) {
  let browser, proxy;
  const contexts = new Map();
  async function release(key, expected) {
    const pending = contexts.get(key);
    if (expected && await pending !== expected || contexts.get(key) !== pending) return;
    contexts.delete(key);
    if (pending) { const owned = await pending; clearTimeout(owned.timer); await owned.context.close(); }
  }
  async function acquire(key) {
    if (!contexts.has(key)) {
      if (contexts.size >= capacity) throw new Error("browser service is at capacity");
      contexts.set(key, (async () => {
        const context = await browser.newContext({ acceptDownloads: false, serviceWorkers: "block" });
        try {
          await context.routeWebSocket("**/*", socket => socket.close());
          const page = await context.newPage();
          page.setDefaultTimeout(10000);
          context.on("page", extra => { if (extra !== page) void extra.close(); });
          return { context, page, busy: false, timer: undefined };
        } catch (error) { await context.close(); throw error; }
      })());
    }
    try { return await contexts.get(key); } catch (error) { contexts.delete(key); throw error; }
  }
  const server = toolServer({ token, capacity, async invoke(route, request, signal) {
    if (route !== "/browser") return failure("unknown browser tool");
    const args = request.arguments, key = JSON.stringify([request.tenant_id, request.session_id]);
    if (!args || !descriptor.input_schema.properties.action.enum.includes(args.action) || Object.keys(args).some(name => !["action", "url", "selector", "value"].includes(name))) return failure("invalid browser arguments");
    if (args.action === "close") { await release(key); return result({ closed: true }); }
    if (["click", "fill"].includes(args.action) && (typeof args.selector !== "string" || !args.selector || args.selector.length > 1024) || args.action === "fill" && (typeof args.value !== "string" || args.value.length > 32768)) return failure("invalid browser input");
    if (args.action === "navigate") {
      let url;
      try { url = new URL(args.url); } catch { return failure("invalid browser URL"); }
      if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || !proxy.allowed(url)) return failure("browser URL is not authorized");
    }
    let owned, dispatched = false, locked = false;
    const abort = () => { void release(key).catch(() => {}); };
    try {
      owned = await acquire(key);
      if (owned.busy) return failure("this session already has an active browser action");
      owned.busy = true; locked = true; clearTimeout(owned.timer);
      signal.throwIfAborted(); signal.addEventListener("abort", abort, { once: true });
      dispatched = true;
      const page = owned.page;
      if (args.action === "navigate") {
        const response = await page.goto(args.url, { waitUntil: "domcontentloaded" });
        if (!response?.ok()) return failure("browser navigation returned an HTTP error");
      }
      else if (args.action === "click") await page.locator(args.selector).click();
      else if (args.action === "fill") await page.locator(args.selector).fill(args.value);
      else if (args.action === "screenshot") {
        const png = await page.screenshot();
        if (png.length > 1 << 20) throw new Error("browser screenshot exceeds the output limit");
        return { content: [{ type: "image", mime_type: "image/png", data: png.toString("base64") }] };
      }
      const text = await page.locator(args.action === "read" && args.selector ? args.selector : "body").innerText();
      if (text.length > 32768) throw new Error("browser text exceeds the output limit");
      return result({ url: page.url(), title: await page.title(), text });
    } catch { return failure("browser action failed; inspect the session before retrying", dispatched); }
    finally {
      signal.removeEventListener("abort", abort);
      if (locked) { owned.busy = false; owned.timer = setTimeout(() => { void release(key, owned).catch(() => {}); }, idleMs); owned.timer.unref(); }
    }
  } });
  proxy = await originProxy(origins);
  try { browser = await chromium.launch({ headless: true, proxy: { server: proxy.url }, args: ["--proxy-bypass-list=<-loopback>"] }); }
  catch (error) { await proxy.close(); throw error; }
  return { server, async close() { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); await Promise.all([...contexts.keys()].map(key => release(key))); await browser.close(); await proxy.close(); } };
}
