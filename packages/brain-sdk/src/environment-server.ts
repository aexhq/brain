import { createServer, type IncomingHttpHeaders } from "node:http";
import { once } from "node:events";
import { z } from "zod";
import { BrainError } from "./errors.js";

export interface EnvironmentServerOptions<Context> {
  token?: string;
  authenticate?: (headers: IncomingHttpHeaders) => Context | Promise<Context>;
  /** Optional provider relay. It must authenticate its own invocation credentials. */
  callback?: (headers: IncomingHttpHeaders, input: unknown) => unknown | Promise<unknown>;
  host?: string;
  port?: number;
  maxBodyBytes?: number;
}

/** Serve an environmentProtocol handler or the run method of a typed environmentHandler. */
export async function serveEnvironment<Context = undefined>(
  handle: (command: unknown, context: Context) => unknown | Promise<unknown>,
  { token, authenticate, callback, host = "127.0.0.1", port = 0, maxBodyBytes = 16 * 1024 * 1024 }: EnvironmentServerOptions<Context> = {},
) {
  if (authenticate === undefined) z.string().min(1).parse(token);
  z.number().int().positive().safe().parse(maxBodyBytes);
  const server = createServer(async (request, response) => {
    const isCallback = request.url === "/v1/callback" && callback !== undefined;
    let context: Context;
    if (!isCallback) {
      try {
        if (authenticate !== undefined) context = await authenticate(request.headers);
        else if (request.headers.authorization !== `Bearer ${token}`) throw new Error("denied");
      } catch { request.resume(); response.writeHead(401).end(); return; }
    }
    if (request.method !== "POST" || (!isCallback && request.url !== "/v1/operations")) { request.resume(); response.writeHead(404).end(); return; }
    try {
      let bytes = 0;
      const chunks: Buffer[] = [];
      for await (const chunk of request) {
        bytes += chunk.length;
        if (bytes > maxBodyBytes) { response.writeHead(413).end(); return; }
        chunks.push(chunk);
      }
      const input: unknown = JSON.parse(Buffer.concat(chunks).toString("utf8"));
      const body = isCallback ? await callback!(request.headers, input) : await handle(input, context!);
      response.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(body));
    } catch (error) {
      response.writeHead(error instanceof BrainError && error.code === "denied" ? 403 : error instanceof SyntaxError || error instanceof z.ZodError ? 400 : 500,
        { "content-type": "application/json" }).end(JSON.stringify({ error: (error instanceof Error ? error.message : String(error)).slice(0, 4096) }));
    }
  });
  server.listen(port, host);
  await once(server, "listening");
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("Environment listener has no TCP address");
  return { url: `http://${host.includes(":") ? `[${host}]` : host}:${address.port}`,
    close: () => new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve())) };
}
