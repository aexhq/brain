import { Miniflare } from "miniflare";
import { randomBytes, randomUUID, createHmac } from "node:crypto";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { parse, stringify } from "lossless-json";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

export async function startD1(directory) {
  const key = randomBytes(32).toString("hex");
  const runtime = new Miniflare({
    host: "127.0.0.1", port: 0, modules: true, cf: false, compatibilityDate: "2026-07-30",
    rootPath: root, scriptPath: path.join(root, "packages", "brain-d1", "dist", "worker.mjs"),
    d1Databases: { DB: randomUUID() }, d1Persist: path.join(directory, "d1"),
    bindings: { BRAIN_D1_KEY: key },
  });
  try {
    const origin = (await runtime.ready).origin;
    const database = await runtime.getD1Database("DB");
    async function rpc(method, args = [], options = {}) {
      const route = "/control/v1/" + method;
      const stamp = String(options.time ?? Date.now());
      const body = JSON.stringify({ arguments: args.map(value => stringify(value)), ...(options.authority ? { authority: options.authority } : {}) });
      const signature = createHmac("sha256", Buffer.from(key, "hex"))
        .update("brain-d1/1\0" + route + "\0" + stamp + "\0" + body).digest("hex");
      const response = await fetch(origin + route, { method: "POST", headers: {
        "Content-Type": "application/json", "X-Brain-Request-Time": stamp, "X-Brain-Control-Signature": signature, ...options.headers,
      }, body: options.body ?? body });
      const text = await response.text();
      return { status: response.status, data: parse(text), text };
    }
    return { origin, key, database, rpc, dispose: () => runtime.dispose() };
  } catch (error) {
    await runtime.dispose();
    throw error;
  }
}
