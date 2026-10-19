import http from "node:http";
import net from "node:net";

// Browser routing hooks do not intercept every redirect. Enforce the deployment's
// exact origin policy at the browser's network exit, including CONNECT tunnels.
export async function originProxy(origins) {
  if (!Array.isArray(origins) || !origins.length || origins.some(value => { const url = new URL(value); return !["http:", "https:"].includes(url.protocol) || url.origin !== value; })) throw new Error("browser origins must be a nonempty exact HTTP/HTTPS origin allowlist");
  const allowed = url => origins.includes(url.origin);
  const sockets = new Set();
  const server = http.createServer((req, res) => {
    let url;
    try { url = new URL(req.url); } catch { res.writeHead(400).end(); return; }
    if (url.protocol !== "http:" || !allowed(url) || url.username || url.password) { res.writeHead(403).end(); return; }
    const { "proxy-connection": _, "proxy-authorization": __, ...headers } = req.headers;
    const outgoing = http.request(url, { method: req.method, headers, agent: false }, incoming => { res.writeHead(incoming.statusCode, incoming.headers); incoming.pipe(res); });
    outgoing.on("error", () => res.destroy()); res.on("close", () => outgoing.destroy()); req.pipe(outgoing);
  });
  server.on("connect", (req, socket, head) => {
    let url;
    try { url = new URL(`https://${req.url}`); } catch { socket.destroy(); return; }
    if (!allowed(url) || url.username || url.password || url.pathname !== "/") { socket.end("HTTP/1.1 403 Forbidden\r\n\r\n"); return; }
    const remote = net.connect(Number(url.port || 443), url.hostname);
    sockets.add(remote); remote.on("close", () => sockets.delete(remote));
    remote.on("connect", () => { socket.write("HTTP/1.1 200 Connection Established\r\n\r\n"); if (head.length) remote.write(head); socket.pipe(remote); remote.pipe(socket); });
    remote.on("error", () => socket.destroy()); socket.on("error", () => remote.destroy()); socket.on("close", () => remote.destroy()); remote.on("close", () => socket.destroy());
  });
  server.on("connection", socket => { sockets.add(socket); socket.on("close", () => sockets.delete(socket)); });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  return { allowed, url: `http://127.0.0.1:${server.address().port}`, async close() { for (const socket of sockets) socket.destroy(); await new Promise(resolve => server.close(resolve)); } };
}
