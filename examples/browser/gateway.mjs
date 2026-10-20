import { randomBytes } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

const secret = () => randomBytes(32).toString("base64url");
const deny = (status = 403) => Object.assign(new Error("browser request denied"), { status });
async function json(req) {
  const chunks = []; let size = 0;
  for await (const chunk of req) { size += chunk.length; if (size > 2 << 20) throw deny(413); chunks.push(chunk); }
  if (!size) return undefined;
  try { return JSON.parse(Buffer.concat(chunks)); } catch { throw deny(400); }
}

/** Platform code authenticates the user before calling issue(). No public issuer. */
export function browserGateway({ origin, brainURL, brainToken }) {
  if (new URL(origin).origin !== origin || !brainToken) throw new Error("gateway origin and backend Brain authority are required");
  const authorities = new Map(), grants = new Set();
  async function upstream(route, method, body, token = brainToken, signal, accept = "application/json") {
    const target = new URL(brainURL);
    const query = route.indexOf("?");
    target.pathname = query < 0 ? route : route.slice(0, query);
    target.search = query < 0 ? "" : route.slice(query);
    return fetch(target, { method, headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json", Accept: accept }, ...(body === undefined ? {} : { body: JSON.stringify(body) }), redirect: "error", signal: signal ?? AbortSignal.timeout(30000) });
  }
  async function revoke(token) {
    const grant = authorities.get(token)?.grant;
    if (!grant) return;
    clearTimeout(grant.timer); grants.delete(grant);
    for (const authority of grant.authorities) authorities.delete(authority);
    for (const controller of grant.active) controller.abort();
    await Promise.all([...grant.executors.values()].map(async executor => {
      const response = await upstream(`/executors/${executor.id}/close`, "POST", undefined, executor.token);
      if (!response.ok && ![401, 403].includes(response.status)) throw new Error("upstream executor revocation failed");
    }));
  }
  function issue({ create, tools = [], session, ttlMs = 300000 }) {
    if (!Number.isSafeInteger(ttlMs) || ttlMs <= 0 || ttlMs > 900000) throw new Error("browser grant lifetime must be at most 15 minutes");
    if (create && (Object.keys(create).some(key => !["harness", "config"].includes(key)) || !create.config)) throw new Error("browser composition must contain public harness/config only");
    const token = secret(), grant = { create: structuredClone(create), tools: structuredClone(tools), session, expires: Date.now() + ttlMs, authorities: new Set([token]), executors: new Map(), active: new Set(), creating: false };
    authorities.set(token, { grant }); grants.add(grant);
    grant.timer = setTimeout(() => { void revoke(token).catch(() => {}); }, ttlMs); grant.timer.unref();
    return { token, expires: grant.expires };
  }
  async function handle(req, res) {
    let grant, controller;
    try {
      // Bearer grants are used instead of cookies. Check Origin before proxying mutations or streams.
      if (req.headers.origin !== origin && !(req.method === "GET" && !req.headers.origin && req.headers["sec-fetch-site"] === "same-origin")) throw deny();
      const supplied = /^Bearer ([A-Za-z0-9_-]+)$/.exec(req.headers.authorization ?? "")?.[1];
      const authority = authorities.get(supplied); grant = authority?.grant;
      if (!grant || Date.now() >= grant.expires) throw deny(401);
      controller = new AbortController(); grant.active.add(controller);
      res.on("close", () => controller.abort());
      const url = new URL(req.url, origin);
      if (url.search && !(req.method === "GET" && url.pathname.endsWith("/events") && [...url.searchParams.keys()].every(key => ["after", "limit"].includes(key)))) throw deny();
      const body = req.method === "POST" ? await json(req) : undefined;
      let response;
      if (url.pathname === "/composition" && req.method === "GET" && !authority.executor) {
        res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
        res.end(JSON.stringify({ create: grant.create, tools: grant.tools, session: grant.session })); return;
      }
      if (url.pathname === "/executors" && req.method === "POST" && !authority.executor) {
        if (body?.binding !== "runtime" || !isDeepStrictEqual(body.tools, grant.tools) || body.session !== grant.session || Object.keys(body).some(key => !["binding", "tools", "session"].includes(key))) throw deny();
        response = await upstream("/executors", "POST", body, brainToken, controller.signal);
        if (response.ok) {
          const executor = await response.json(), token = secret();
          if (!authorities.has(supplied)) { await upstream(`/executors/${executor.id}/close`, "POST", undefined, executor.token); throw deny(401); }
          grant.executors.set(executor.id, executor); grant.authorities.add(token); authorities.set(token, { grant, executor });
          res.writeHead(201, { "content-type": "application/json", "cache-control": "no-store" });
          res.end(JSON.stringify({ id: executor.id, token })); return;
        }
      } else if (url.pathname === "/sessions" && req.method === "POST" && !authority.executor) {
        if (grant.session || grant.creating || !grant.create) throw deny();
        const bindings = [...(grant.create.config.bindings ?? [])];
        const declared = body?.config?.bindings?.filter(binding => binding.alias === "runtime") ?? [];
        if (grant.tools.length) {
          if (declared.length !== 1 || !grant.executors.has(declared[0].executor)) throw deny();
          bindings.push({ alias: "runtime", executor: declared[0].executor, runtime: grant.tools });
        }
        const expected = { ...grant.create, config: { ...grant.create.config, ...(bindings.length ? { bindings } : {}) } };
        if (!isDeepStrictEqual(body, expected)) throw deny();
        grant.creating = true;
        try {
          response = await upstream("/sessions", "POST", body, brainToken, controller.signal);
          if (response.ok) { const session = await response.json(); grant.session = session.id; response = Response.json(session, { status: response.status }); }
        } finally { grant.creating = false; }
      } else if (authority.executor) {
        const executor = authority.executor;
        if (!new RegExp(`^/executors/${executor.id}/(commands|responses|close)$`).test(url.pathname) || (url.pathname.endsWith("/commands") ? req.method !== "GET" : req.method !== "POST")) throw deny();
        response = await upstream(url.pathname, req.method, body, executor.token, controller.signal, req.headers.accept);
      } else {
        const match = /^\/sessions\/([a-f0-9]{32})(?:\/(events|resume|close))?$/.exec(url.pathname);
        if (!match || match[1] !== grant.session || !["GET", "POST"].includes(req.method) || (req.method === "GET" ? ![undefined, "events"].includes(match[2]) : !["events", "resume", "close"].includes(match[2]))) throw deny();
        response = await upstream(url.pathname + url.search, req.method, body, brainToken, controller.signal, req.headers.accept);
      }
      res.writeHead(response.status, { "content-type": response.headers.get("content-type") ?? "application/json", "cache-control": "no-store", ...(response.headers.has("x-request-id") ? { "x-request-id": response.headers.get("x-request-id") } : {}) });
      if (response.body) await pipeline(Readable.fromWeb(response.body), res); else res.end();
    } catch (error) {
      if (!res.headersSent && !res.destroyed) { res.writeHead(error.status ?? 502, { "content-type": "application/json", "cache-control": "no-store" }); res.end(JSON.stringify({ error: { code: "browser_gateway_error", message: "browser request denied or upstream unavailable" } })); }
      else res.destroy();
    } finally { grant?.active.delete(controller); }
  }
  return { issue, revoke, handle, async close() { await Promise.all([...grants].map(grant => revoke([...grant.authorities][0]))); } };
}
