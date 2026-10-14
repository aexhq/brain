import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { randomUUID } from "node:crypto";
import { createRequire } from "node:module";
import { createInterface } from "node:readline";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { z } from "zod";
import type { AgentHarness, HarnessContext } from "./harness.js";
import { activationText, catalogue, placedTools } from "./harness-bridge.js";
import type { ContentBlock, Message, ModelResult } from "./generated/session.js";

const optionsSchema = z.strictObject({
  contextWindow: z.number().int().positive().optional(),
  compactAtTokens: z.number().int().positive().optional(),
});
// These are private transport objects from the pinned upstream app-server/Responses
// APIs. Brain's generated types remain the public callback contract.
type Wire = Record<string, any>;
const nativeFormat = "openai.responses.v1";
const opaquePrefix = "brain-native:";

function fromInput(items: Wire[]): Message[] {
  return items.map(item => {
    if (item.type === "function_call") return { role: "assistant", content: [
      { type: "tool_use", id: item.call_id, name: item.name, input: JSON.parse(item.arguments) },
    ] };
    if (item.type === "function_call_output") return { role: "user", content: [
      { type: "tool_result", tool_use_id: item.call_id, content: item.output, is_error: false },
    ] };
    if (item.type === "reasoning" || item.type === "compaction") {
      const block = typeof item.encrypted_content === "string" && item.encrypted_content.startsWith(opaquePrefix)
        ? JSON.parse(Buffer.from(item.encrypted_content.slice(opaquePrefix.length), "base64url").toString()) as ContentBlock
        : { type: "native" as const, format: nativeFormat, data: item };
      return { role: "assistant", content: [block] };
    }
    if (item.type !== undefined && item.type !== "message") throw new Error(`unsupported Codex input item: ${item.type}`);
    if (!["user", "assistant", "developer", "system"].includes(item.role)) throw new Error("unsupported Codex message role");
    return { role: item.role === "system" ? "developer" : item.role,
      content: typeof item.content === "string" ? [{ type: "text", text: item.content }] : item.content.map((part: Wire) => {
        if (part.type === "input_text" || part.type === "output_text") return { type: "text", text: part.text };
        if (part.type === "input_image") return { type: "image", url: part.image_url };
        if (part.type === "input_file") return { type: "file", url: part.file_url, media_type: "application/pdf" };
        throw new Error(`unsupported Codex input content: ${part.type}`);
      }) };
  });
}

function outputItems(result: ModelResult): Wire[] {
  return result.message.content.map(block => {
    switch (block.type) {
      case "text": return { type: "message", id: `msg_${randomUUID()}`, role: "assistant", status: "completed",
        content: [{ type: "output_text", text: block.text, annotations: [] }] };
      case "tool_use": return { type: "function_call", id: `fc_${randomUUID()}`, call_id: block.id,
        name: block.name, arguments: JSON.stringify(block.input), status: "completed" };
      case "native": return block.format === nativeFormat ? block.data as Wire : {
        type: "reasoning", id: `rs_${randomUUID()}`, summary: [],
        encrypted_content: opaquePrefix + Buffer.from(JSON.stringify(block)).toString("base64url"),
      };
      default: throw new Error(`unsupported Codex assistant content: ${block.type}`);
    }
  });
}

/** The pinned upstream Codex app-server owns its loop, rollout files, and compaction. */
export function codexHarness(): AgentHarness {
  return {
    name: "codex", stateVersion: "1",
    async open({ stateDirectory, configuration }) {
      let process: ChildProcessWithoutNullStreams | undefined;
      let current: HarnessContext | undefined;
      let threadId: string | undefined;
      let turnId: string | undefined;
      let initialCatalogue: string | undefined;
      let completion: ReturnType<typeof Promise.withResolvers<void>> | undefined;
      let failure: unknown;
      let projection: Message[] = [];
      let next = 1;
      const pending = new Map<number, ReturnType<typeof Promise.withResolvers<Wire>>>();
      const token = randomUUID();
      const tools = () => placedTools(active().tools).map(tool => ({ ...tool, name: `brain_${tool.name}` }));
      const active = () => {
        if (failure !== undefined) throw failure;
        if (current === undefined) throw new Error("Codex attempted execution outside an active Brain activation");
        current.signal.throwIfAborted();
        return current;
      };
      const fail = (error: unknown) => {
        failure ??= error;
        completion?.reject(error);
      };
      const write = (frame: Wire) => {
        if (!process?.stdin.writable) throw new Error("Codex app-server is closed");
        process.stdin.write(`${JSON.stringify(frame)}\n`);
      };
      const request = (method: string, params: Wire) => {
        if (failure !== undefined) return Promise.reject(failure);
        const id = next++, reply = Promise.withResolvers<Wire>();
        pending.set(id, reply);
        try { write({ id, method, params }); } catch (error) { pending.delete(id); reply.reject(error); }
        return reply.promise;
      };
      async function toolCall(frame: Wire) {
        try {
          const ctx = active(), params = frame.params;
          if (frame.method !== "item/tool/call" || params.threadId !== threadId || params.turnId !== turnId || params.namespace != null) {
            throw new Error(`unsupported Codex callback: ${frame.method}`);
          }
          const tool = tools().find(tool => tool.name === params.tool);
          if (tool === undefined) throw new Error(`Codex requested an unoffered tool: ${params.tool}`);
          const result = await tool.execute(ctx, params.callId, params.arguments);
          if (result.media?.length) throw new Error("Codex harness does not support Tool media; use Pi");
          write({ id: frame.id, result: { success: !result.is_error,
            contentItems: [{ type: "inputText", text: typeof result.content === "string" ? result.content : JSON.stringify(result.content) }] } });
        } catch (error) {
          fail(error);
          write({ id: frame.id, error: { code: -32000, message: error instanceof Error ? error.message : String(error) } });
        }
      }
      async function gateway(req: IncomingMessage, res: ServerResponse) {
        try {
          if (req.headers.authorization !== `Bearer ${token}`) { res.writeHead(401).end(); return; }
          const compact = req.url === "/responses/compact";
          if (req.method !== "POST" || (!compact && req.url !== "/responses")) throw new Error("unsupported Codex provider operation");
          const ctx = active();
          const chunks = [];
          for await (const chunk of req) chunks.push(chunk as Buffer);
          const body = JSON.parse(Buffer.concat(chunks).toString()) as Wire;
          const admitted = new Set(tools().map(tool => tool.name));
          // Codex can declare native utilities even with environments disabled. They
          // never reach the model, and their calls are rejected before Codex sees them.
          const offered = (body.tools ?? []).filter((tool: Wire) => tool.type === "function" && admitted.has(tool.name));
          const messages = fromInput(body.input);
          const result = await ctx.model({ messages, system: body.instructions ?? "", tools: compact ? [] : offered.map((tool: Wire) => ({
            name: tool.name, description: tool.description ?? "", input_schema: tool.parameters,
          })), ...(compact ? { options: { operation: "compact" } } : {
            ...(body.max_output_tokens === undefined ? {} : { max_output_tokens: body.max_output_tokens }),
            ...(body.text?.format === undefined ? {} : { response_format: body.text.format }),
            options: Object.fromEntries(["reasoning", "temperature", "top_p"].filter(key => body[key] !== undefined).map(key => [key, body[key]])),
          }) });
          const output = outputItems(result);
          for (const item of output) {
            if (item.type === "function_call" && !offered.some((tool: Wire) => tool.name === item.name)) {
              throw new Error(`model requested an unoffered tool: ${item.name}`);
            }
            if (!["message", "reasoning", "compaction", "function_call", ...(compact ? ["function_call_output"] : [])].includes(item.type)) {
              throw new Error(`unsupported Codex provider output: ${item.type}`);
            }
          }
          // Codex needs numbers for its private context budget. Journal usage is the
          // original Brain result; missing counters are never filled in that record.
          const input = result.usage.total_input_tokens ?? result.usage.input_tokens ?? Math.ceil(JSON.stringify(body.input).length / 4);
          const out = result.usage.output_tokens ?? 0;
          const response = { id: `resp_${randomUUID()}`, object: compact ? "response.compaction" : "response", created_at: Math.floor(Date.now() / 1000),
            status: "completed", output, usage: { input_tokens: input, output_tokens: out, total_tokens: input + out,
              input_tokens_details: { cached_tokens: result.usage.cache_read_input_tokens ?? 0 },
              output_tokens_details: { reasoning_tokens: result.usage.reasoning_tokens ?? 0 } } };
          if (compact) { res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(response)); return; }
          projection = [...messages, result.message];
          res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache" });
          const emit = (event: Wire) => res.write(`event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
          emit({ type: "response.created", response: { ...response, output: [], status: "in_progress" } });
          output.forEach((item, index) => {
            emit({ type: "response.output_item.added", output_index: index, item });
            if (item.type === "message") item.content.forEach((part: Wire, content_index: number) => {
              if (part.type === "output_text") emit({ type: "response.output_text.delta", item_id: item.id, output_index: index, content_index, delta: part.text });
            });
            emit({ type: "response.output_item.done", output_index: index, item });
          });
          emit({ type: "response.completed", response });
          res.end();
        } catch (error) {
          fail(error);
          res.writeHead(400, { "content-type": "application/json" }).end(JSON.stringify({ error: { message: error instanceof Error ? error.message : String(error), type: "invalid_request_error" } }));
        }
      }
      const server = createServer((req, res) => { void gateway(req, res); });
      const exited = Promise.withResolvers<void>();
      let closing: Promise<void> | undefined;
      async function start(ctx: HarnessContext) {
        const options = optionsSchema.parse({ ...(configuration as object ?? {}), ...(ctx.configuration as object ?? {}) });
        const contextWindow = options.contextWindow ?? ctx.modelInfo.context_window_tokens;
        if (contextWindow === undefined) throw new Error("Codex requires contextWindow model metadata");
        initialCatalogue = catalogue(ctx.tools);
        if (tools().some(tool => !/^[A-Za-z0-9_-]{1,128}$/u.test(tool.name))) throw new Error("Codex tool names must contain only letters, digits, underscores, and hyphens, up to 122 characters");
        const home = join(stateDirectory, "codex");
        await mkdir(home, { recursive: true });
        await new Promise<void>((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
        const address = server.address();
        if (address === null || typeof address === "string") throw new Error("Codex provider did not bind TCP");
        const { platform, arch } = globalThis.process;
        const target = { linux: "unknown-linux-musl", darwin: "apple-darwin", win32: "pc-windows-msvc" }[platform as "linux" | "darwin" | "win32"];
        const architecture = { x64: "x86_64", arm64: "aarch64" }[arch as "x64" | "arm64"];
        if (!target || !architecture) throw new Error(`unsupported Codex platform: ${platform}/${arch}`);
        const packagePath = createRequire(import.meta.url).resolve(`@openai/codex-${platform}-${arch}/package.json`);
        const binary = join(dirname(packagePath), "vendor", `${architecture}-${target}`, "bin", platform === "win32" ? "codex.exe" : "codex");
        const env = { ...globalThis.process.env, CODEX_HOME: home, BRAIN_HARNESS_TOKEN: token,
          OPENAI_API_KEY: "", OPENAI_BASE_URL: "", CODEX_API_KEY: "" };
        process = spawn(binary, ["app-server", "--stdio"], { cwd: stateDirectory, env, windowsHide: true, stdio: "pipe" });
        process.stderr.on("data", chunk => globalThis.process.stderr.write(chunk));
        process.on("error", error => { fail(error); for (const reply of pending.values()) reply.reject(error); exited.resolve(); });
        process.on("close", () => {
          const error = new Error("Codex app-server exited; operations are not replayed");
          fail(error); for (const reply of pending.values()) reply.reject(error); pending.clear(); exited.resolve();
        });
        process.stdin.on("error", fail);
        createInterface({ input: process.stdout }).on("line", line => {
          try {
            const frame = JSON.parse(line) as Wire;
            if (frame.method && frame.id !== undefined) { void toolCall(frame).catch(fail); return; }
            if (frame.id !== undefined) {
              const reply = pending.get(frame.id);
              if (!reply) throw new Error("unknown Codex request id");
              pending.delete(frame.id);
              if (frame.error) reply.reject(new Error(frame.error.message)); else reply.resolve(frame.result);
            } else if (frame.method === "turn/started" && frame.params.threadId === threadId) turnId = frame.params.turn.id;
            else if (frame.method === "turn/completed" && frame.params.threadId === threadId) {
              if (frame.params.turn.status === "completed") completion?.resolve();
              else fail(new Error(`Codex turn ${frame.params.turn.status}: ${JSON.stringify(frame.params.turn.error)}`));
            }
          } catch (error) { fail(error); }
        });
        await request("initialize", { clientInfo: { name: "brain", version: "1" }, capabilities: { experimentalApi: true } });
        write({ method: "initialized", params: {} });
        const pointer = join(stateDirectory, "native-session.json");
        try { threadId = JSON.parse(await readFile(pointer, "utf8")) as string; }
        catch (error) { if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error; }
        if (threadId === undefined && ctx.transcript.length) throw new Error("Codex starts from native state; importing a Brain transcript is unsupported");
        const params = {
          model: "brain", modelProvider: "brain", cwd: stateDirectory, approvalPolicy: "never", sandbox: "read-only",
          baseInstructions: ctx.system || "Use the provided tools to help the user.", environments: [], selectedCapabilityRoots: [],
          config: { model_context_window: contextWindow, model_auto_compact_token_limit: options.compactAtTokens ?? Math.floor(contextWindow * 0.8),
            project_doc_max_bytes: 0, web_search: "disabled", mcp_servers: {},
            features: { shell_tool: false, view_image: false, multi_agent: false, apps: false, responses_websockets_v2: false },
            model_providers: { brain: { name: "Brain", base_url: `http://127.0.0.1:${address.port}`,
              wire_api: "responses", env_key: "BRAIN_HARNESS_TOKEN", request_max_retries: 0, stream_max_retries: 0, supports_websockets: false } } },
        };
        const opened = await request(threadId === undefined ? "thread/start" : "thread/resume", threadId === undefined
          ? { ...params, dynamicTools: tools().map(tool => ({ type: "function", name: tool.name, description: tool.description, inputSchema: tool.input_schema })) }
          : { ...params, threadId, excludeTurns: true });
        threadId = opened.thread.id;
        await writeFile(pointer, JSON.stringify(threadId));
      }
      function close() {
        return closing ??= (async () => {
          if (process) {
            process.stdin.end();
            const timer = setTimeout(() => process?.kill(), 5000);
            try { await exited.promise; } finally { clearTimeout(timer); process = undefined; }
          }
          if (server.listening) await new Promise<void>((resolve, reject) => {
            server.close(error => error ? reject(error) : resolve());
            server.closeAllConnections();
          });
        })();
      }
      return {
        async run(ctx) {
          if (failure !== undefined) throw failure;
          current = ctx; projection = [];
          completion = Promise.withResolvers<void>();
          // Attach before startup; an app-server failure can arrive while awaiting RPC.
          void completion.promise.catch(() => undefined);
          try {
            if (ctx.input?.media?.length) throw new Error("Codex harness currently supports text inputs; use Pi for URL media");
            if (!process) await start(ctx);
            if (catalogue(ctx.tools) !== initialCatalogue) throw new Error("Codex requires fixed Tool placements for the native session");
            const text = await activationText(ctx);
            if (text === undefined) return;
            const result = await request("turn/start", { threadId, environments: [],
              input: [{ type: "text", text, text_elements: [] }] });
            turnId = result.turn.id;
            await completion.promise;
            if (failure !== undefined) throw failure;
            ctx.signal.throwIfAborted();
            if (projection.length) await ctx.setTranscript(projection);
            const message = projection.findLast(message => message.role === "assistant")?.content
              .filter(block => block.type === "text").map(block => block.text).join("");
            if (message) await ctx.emit("output_emitted", { type: "assistant_message", message });
          } catch (error) { await close(); throw error; }
          finally { current = undefined; completion = undefined; turnId = undefined; }
        },
        async cancel() {
          if (process && threadId && turnId) {
            try { await request("turn/interrupt", { threadId, turnId }); }
            catch (error) {
              // A cancelled Brain callback may already be closing app-server.
              // Await that cleanup; run() retains the original activation failure.
              if (closing === undefined) throw error;
              await closing;
            }
          }
        },
        close,
      };
    },
  };
}
