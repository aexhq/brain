import { createServer } from "node:http";

let models = 0, effects = 0, hold = false;
const waiting = new Set();
const server = createServer(async (request, response) => {
  let raw = "";
  for await (const chunk of request) {
    raw += chunk;
    if (Buffer.byteLength(raw) > 1048576) { response.writeHead(413).end(); return; }
  }
  response.setHeader("Content-Type", "application/json");
  if (request.url === "/health/live") { response.writeHead(204).end(); return; }
  if (request.url === "/state") { response.end(JSON.stringify({ models, effects })); return; }
  if (request.url === "/hold" && request.method === "POST") { hold = true; response.end("{}"); return; }
  if (request.url === "/release" && request.method === "POST") {
    hold = false;
    for (const complete of waiting) complete();
    waiting.clear();
    response.end("{}"); return;
  }
  if (request.url === "/effect" && request.method === "POST") {
    effects++;
    response.end(JSON.stringify({ delivered: effects })); return;
  }
  if (request.url === "/v1/chat/completions" && request.method === "POST") {
    const input = JSON.parse(raw);
    if (!Array.isArray(input.messages)) { response.writeHead(400).end(); return; }
    const count = ++models;
    const complete = () => response.end(JSON.stringify({
      id: `fixture-${count}`, choices: [{ message: { role: "assistant", content: `model result ${count}`, tool_calls: [] } }],
      usage: { prompt_tokens: 7 },
    }));
    if (hold) waiting.add(complete);
    else complete();
    return;
  }
  response.writeHead(404).end("{}");
});
server.listen(9000, "0.0.0.0");
