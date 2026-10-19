import { randomBytes } from "node:crypto";
const maxFrame = 4 << 20;
export const identity = () => randomBytes(16).toString("hex");
export const unknownUsage = () => ({ availability: "unknown", semantics: "snapshot", input_cache: "unknown", output_reasoning: "unknown" });
export function fatal() { process.stderr.write("Brain native runner stopped\n"); process.exit(1); }
export function send(value) {
  const line = JSON.stringify(value) + "\n";
  if (Buffer.byteLength(line) > maxFrame || process.stdout.writableLength > 2 * maxFrame) fatal();
  process.stdout.write(line);
}
export function lines(stream, receive) {
  let buffer = Buffer.alloc(0);
  stream.on("data", chunk => {
    buffer = Buffer.concat([buffer, chunk]);
    let end;
    while ((end = buffer.indexOf(10)) >= 0) {
      if (end > maxFrame) fatal();
      const line = buffer.subarray(0, end); buffer = buffer.subarray(end + 1);
      try { void Promise.resolve(receive(JSON.parse(line))).catch(fatal); } catch { fatal(); }
    }
    if (buffer.length > maxFrame) fatal();
  });
}

export class RunnerSession {
  turn = "";
  busy = false;
  pending = new Map();
  constructor(attachment) { this.attachment = attachment; }
  event(event, data) { send({ attachment: this.attachment, turn: this.turn, event, data }); }
  request(method, params) {
    if (this.failure) return Promise.reject(this.failure);
    const id = identity(), turn = this.turn;
    return new Promise((resolve, reject) => { this.pending.set(id, { resolve, reject, turn }); send({ attachment: this.attachment, turn, id, method, params }); });
  }
  observe(request) { return this.request("model.request", request); }
  tool(call) { return this.request("tool.call", call); }
  reply(frame) {
    const pending = this.pending.get(frame.id);
    if (!pending || frame.turn !== pending.turn) fatal();
    this.pending.delete(frame.id);
    if (frame.error) { this.failure = new Error("Brain callback failed"); pending.reject(this.failure); } else pending.resolve(frame.result);
  }
}

export function serve(create) {
  const sessions = new Map();
  lines(process.stdin, async frame => {
    if (!frame.attachment || !frame.id) fatal();
    let session = sessions.get(frame.attachment);
    if (frame.method === "open") {
      if (session) fatal();
      session = create(frame.attachment); sessions.set(frame.attachment, session);
    }
    if (!session) fatal();
    if (!frame.method) { session.reply(frame); return; }
    try {
      let result;
      if (frame.method === "open") result = await session.open(frame.params);
      else if (frame.method === "prompt") {
        if (session.busy || !frame.turn) throw new Error("session already active");
        session.busy = true; session.turn = frame.turn; session.failure = undefined;
        try { result = await session.prompt(frame.params.text); if (session.failure) throw session.failure; } finally { session.busy = false; }
      } else if (frame.method === "interrupt") result = await session.interrupt();
      else if (frame.method === "checkpoint" || frame.method === "detach") {
        if (session.busy || session.pending.size) throw new Error("session is active");
        result = await session[frame.method]();
        if (frame.method === "detach") sessions.delete(frame.attachment);
      } else throw new Error("unsupported native operation");
      send({ attachment: frame.attachment, turn: frame.turn, id: frame.id, result: result ?? {} });
    } catch (error) {
      if (session.failure) error = session.failure;
      send({ attachment: frame.attachment, turn: frame.turn, id: frame.id, error: { code: ["provider_error", "unsupported", "invalid_request", "forbidden", "overloaded"].includes(error.code) ? error.code : "brain_error", message: error.safeMessage ?? `native ${frame.method} failed: ${error.name}/${error.code ?? ""}` } });
    } finally { if (frame.method === "prompt") session.turn = ""; }
  });
  process.stdin.on("end", () => process.exit(0));
  process.stdin.on("error", fatal); process.stdout.on("error", fatal); process.on("unhandledRejection", fatal);
}
