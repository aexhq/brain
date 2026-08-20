import type { Tool, ToolContext } from "./tools.js";

interface CallFrame {
  type: "call";
  call_id: string;
  callback_id: string;
  name: string;
  input: unknown;
}

interface AbortFrame {
  type: "abort";
  call_id: string;
}

interface ReadyFrame {
  type: "ready";
}

interface ErrorFrame {
  type: "error";
  message: string;
}

type ServerFrame = CallFrame | AbortFrame | ReadyFrame | ErrorFrame;

export type WebSocketFactory = (url: string) => WebSocket;

/** One explicitly attached callback worker for one durable Brain session. */
export class AttachedWorker {
  readonly #socket: WebSocket;
  readonly #tools: ReadonlyMap<string, Tool>;
  readonly #controllers = new Map<string, AbortController>();
  readonly #results = new Map<string, string>();
  readonly ready: Promise<void>;

  constructor(
    url: string,
    token: string,
    tools: ReadonlyMap<string, Tool>,
    factory: WebSocketFactory,
  ) {
    this.#tools = tools;
    this.#socket = factory(url);
    this.ready = new Promise((resolve, reject) => {
      let settled = false;
      const fail = (error: unknown): void => {
        if (settled) return;
        settled = true;
        reject(error instanceof Error ? error : new Error(String(error)));
      };
      this.#socket.addEventListener("open", () => {
        this.#socket.send(JSON.stringify({
          type: "hello",
          token,
          callbacks: [...tools.keys()],
        }));
      });
      this.#socket.addEventListener("message", (event) => {
        let frame: ServerFrame;
        try {
          frame = JSON.parse(String(event.data)) as ServerFrame;
        } catch (cause) {
          fail(new Error("Brain sent an invalid attached-worker frame", { cause }));
          this.close();
          return;
        }
        if (frame.type === "ready") {
          if (!settled) {
            settled = true;
            resolve();
          }
          return;
        }
        if (frame.type === "error") {
          fail(new Error(frame.message));
          this.close();
          return;
        }
        void this.#handle(frame);
      });
      this.#socket.addEventListener("error", () => fail(new Error("Attached worker connection failed")));
      this.#socket.addEventListener("close", () => {
        for (const controller of this.#controllers.values()) controller.abort("attached worker disconnected");
        this.#controllers.clear();
        fail(new Error("Attached worker disconnected before becoming ready"));
      });
    });
  }

  close(): void {
    this.#socket.close();
  }

  async #handle(frame: CallFrame | AbortFrame): Promise<void> {
    if (frame.type === "abort") {
      this.#controllers.get(frame.call_id)?.abort("Brain cancelled the Tool call");
      return;
    }
    const prior = this.#results.get(frame.call_id);
    if (prior !== undefined) {
      this.#socket.send(prior);
      return;
    }
    const tool = this.#tools.get(frame.callback_id);
    if (tool === undefined || tool.name !== frame.name || typeof tool.execute !== "function") {
      this.#send(frame.call_id, false, undefined, "attached Tool identity does not match the session seal");
      return;
    }
    const controller = new AbortController();
    this.#controllers.set(frame.call_id, controller);
    try {
      const input = await tool.input.parseAsync(frame.input);
      const context: ToolContext = {
        signal: controller.signal,
        callId: frame.call_id,
        workspace: attachedWorkspace(),
        deadlineMs: Date.now() + 10 * 60 * 1000,
      };
      const value = await tool.execute(input, context);
      const output = await tool.output.parseAsync(value);
      this.#send(frame.call_id, true, output);
    } catch (error) {
      this.#send(frame.call_id, false, undefined, messageOf(error));
    } finally {
      this.#controllers.delete(frame.call_id);
    }
  }

  #send(callId: string, ok: boolean, output?: unknown, error?: string): void {
    const text = JSON.stringify({
      type: "result",
      call_id: callId,
      ok,
      ...(output === undefined ? {} : { output }),
      ...(error === undefined ? {} : { error: error.slice(0, 16 * 1024) }),
    });
    if (text.length > 128 * 1024) {
      this.#send(callId, false, undefined, "attached Tool result exceeds 128 KiB");
      return;
    }
    this.#results.set(callId, text);
    this.#socket.send(text);
    if (this.#results.size > 256) {
      const oldest = this.#results.keys().next().value as string | undefined;
      if (oldest !== undefined) this.#results.delete(oldest);
    }
  }
}

function attachedWorkspace(): string {
  const processLike = globalThis as typeof globalThis & { process?: { cwd(): string } };
  return processLike.process?.cwd() ?? "/";
}

function messageOf(error: unknown): string {
  return error instanceof Error && error.message !== "" ? error.message : String(error);
}
