import type * as Wire from "./generated/session-v2.js";
export type * from "./generated/session-v2.js";

export class SessionError extends Error {
  constructor(readonly code: string, message: string, readonly status: number, readonly uncertain = false, readonly requestId?: string) {
    super(message);
    this.name = "SessionError";
  }
}

export interface SessionClientOptions {
  url?: string;
  token?: string;
  fetch?: typeof fetch;
}

export type StreamEvent =
  | { kind: "record"; id: string; record: Wire.Record }
  | { kind: "gap"; id: string; gap: Wire.Gap };

export interface ExecutionReceipts {
  begin(execution: Wire.Execution): Promise<void>;
  complete(execution: Wire.Execution, receipt: Wire.Receipt): Promise<void>;
  pending(): AsyncIterable<{ execution: Wire.Execution; receipt?: Wire.Receipt }>;
  acknowledged(operationId: string): Promise<void>;
}

const id = (): string => globalThis.crypto.randomUUID();
const segment = (value: string) => encodeURIComponent(value);

async function boundedBytes(response: Response, limit: number): Promise<Uint8Array> {
  if (Number(response.headers.get("Content-Length")) > limit) {
    await response.body?.cancel();
    throw new SessionError("invalid", "Response exceeds SDK byte limit", response.status);
  }
  const reader = response.body?.getReader();
  if (reader === undefined) return new Uint8Array();
  const chunks: Uint8Array[] = [];
  let size = 0;
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > limit) throw new SessionError("invalid", "Response exceeds SDK byte limit", response.status);
      chunks.push(value);
    }
    const output = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) { output.set(chunk, offset); offset += chunk.byteLength; }
    return output;
  } finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
}

export class SessionClient {
  readonly url: string;
  readonly fetch: typeof fetch;
  private readonly token?: string;

  constructor(options: SessionClientOptions = {}) {
    this.url = (options.url ?? "http://127.0.0.1:8080").replace(/\/$/u, "");
    this.fetch = options.fetch ?? globalThis.fetch;
    this.token = options.token;
  }

  headers(extra: Record<string, string> = {}): Record<string, string> {
    return { ...(this.token === undefined ? {} : { Authorization: `Bearer ${this.token}` }), ...extra };
  }

  async request<T>(method: string, path: string, body?: unknown, options: { token?: string; signal?: AbortSignal; bytes?: boolean; mediaType?: string } = {}): Promise<T> {
    const requestId = typeof body === "object" && body !== null && "request_id" in body && typeof body.request_id === "string" ? body.request_id : undefined;
    let response: Response;
    try {
      response = await this.fetch(`${this.url}${path}`, {
        method, redirect: "error", signal: options.signal,
        headers: this.headers({
          ...(body === undefined ? {} : { "Content-Type": options.mediaType ?? "application/json" }),
          ...(options.token === undefined ? {} : { Authorization: `Bearer ${options.token}` }),
        }),
        body: body === undefined ? undefined : options.bytes ? body as BodyInit : JSON.stringify(body),
      });
    } catch {
      throw new SessionError(method === "GET" ? "unavailable" : "uncertain",
        method === "GET" ? "Read transport failed" : "Mutation may have committed; reconcile its original request_id", 0, method !== "GET", requestId);
    }
    if (!response.ok) {
      const failure = await boundedBytes(response, 65536).then(value => JSON.parse(new TextDecoder().decode(value)))
        .catch(() => ({ code: method === "GET" ? "unavailable" : "uncertain", message: "Invalid server error" })) as Wire.ErrorResponse;
      throw new SessionError(failure.code, failure.message, response.status, failure.code === "uncertain", failure.request_id ?? requestId);
    }
    if (response.status === 204) return undefined as T;
    try {
      const body = await boundedBytes(response, options.bytes && method === "GET" ? 16 * 1024 * 1024 : 4 * 1024 * 1024);
      if (options.bytes && method === "GET") return body as T;
      return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(body)) as T;
    } catch {
      throw new SessionError(method === "GET" ? "unavailable" : "uncertain",
        method === "GET" ? "Response was invalid or exceeded its byte limit" : "Mutation may have committed; its response was unavailable",
        response.status, method !== "GET", requestId);
    }
  }

  async create(config: Wire.SessionConfig, options: { message?: string; requestId?: string } = {}): Promise<Session> {
    const view = await this.request<Wire.SessionView>("POST", "/v2/sessions",
      { config, request_id: options.requestId ?? id(), ...(options.message === undefined ? {} : { message: options.message }) });
    return new Session(this, view.id);
  }

  session(sessionId: string): Session { return new Session(this, sessionId); }
  identity(): Promise<Wire.IdentityView> { return this.request("GET", "/v2/identity"); }
  list(after = "", limit = 64): Promise<Wire.SessionPage> {
    return this.request("GET", `/v2/sessions?after=${segment(after)}&limit=${limit}`);
  }
  async *sessions(): AsyncGenerator<Wire.SessionView> {
    let after = "";
    for (;;) {
      const page = await this.list(after);
      yield* page.sessions;
      if (!page.next) return;
      if (page.next === after) throw new SessionError("corrupt", "Session cursor did not advance", 0);
      after = page.next;
    }
  }
  catalog(): Promise<Wire.CatalogEntry[]> { return this.request("GET", "/v2/catalog"); }
  nodes(after = "", limit = 64): Promise<Wire.NodeInfo[]> {
    return this.request("GET", `/operator/nodes?after=${segment(after)}&limit=${limit}`);
  }
  admitPackage(request: Wire.PackageAdmission): Promise<Wire.Package> { return this.request("POST", "/v2/packages", request); }
  package(packageId: string): Promise<Wire.Package> { return this.request("GET", `/v2/packages/${segment(packageId)}`); }
}

export class Session {
  readonly path: string;
  constructor(readonly client: SessionClient, readonly id: string) { this.path = `/v2/sessions/${segment(id)}`; }
  read(): Promise<Wire.SessionView> { return this.client.request("GET", this.path); }
  input(data: Wire.InputData, requestId = id()): Promise<Wire.CommandReceipt> {
    return this.client.request("POST", `${this.path}/inputs`, { request_id: requestId, data });
  }
  send(text: string, requestId?: string): Promise<Wire.CommandReceipt> { return this.input({ kind: "message", text }, requestId); }
  event(name: string, version: string, data: unknown, options: { requestId?: string; correlation?: string } = {}): Promise<Wire.CommandReceipt> {
    return this.input({ kind: "event", name, version, data, ...options.correlation && { correlation: options.correlation } }, options.requestId);
  }
  invoke(name: string, args: unknown, requestId?: string): Promise<Wire.CommandReceipt> { return this.input({ kind: "action", name, arguments: args }, requestId); }
  control(control: Wire.ControlKind, requestId?: string): Promise<Wire.CommandReceipt> { return this.input({ kind: "control", control }, requestId); }
  command(commandId: string): Promise<Wire.CommandReceipt> { return this.client.request("GET", `${this.path}/commands/${segment(commandId)}`); }
  records(after = "0", limit = 64): Promise<Wire.RecordPage> { return this.client.request("GET", `${this.path}/records?after=${segment(after)}&limit=${limit}`); }
  runs(limit = 64, after = ""): Promise<Wire.Run[]> { return this.client.request("GET", `${this.path}/runs?limit=${limit}&after=${segment(after)}`); }
  children(runId: string, step = 0): Promise<Wire.ChildResults> {
    return this.client.request("GET", `${this.path}/runs/${segment(runId)}/children?step=${step}`);
  }
  childResult(runId: string, child: number, step = 0): Promise<Wire.Record> {
    return this.client.request("GET", `${this.path}/runs/${segment(runId)}/children/${child}/result?step=${step}`);
  }
  operations(limit = 64, after = ""): Promise<Wire.Operation[]> { return this.client.request("GET", `${this.path}/operations?limit=${limit}&after=${segment(after)}`); }
  operation(operationId: string): Promise<Wire.Operation> { return this.client.request("GET", `${this.path}/operations/${segment(operationId)}`); }
  budget(): Promise<Wire.SessionBudget> { return this.client.request("GET", `${this.path}/budget`); }
  claim(operationId: string, request: Wire.Claim): Promise<Wire.Execution> { return this.client.request("POST", `${this.path}/operations/${segment(operationId)}/claim`, request); }
  receipt(operationId: string, receipt: Wire.Receipt, receiptToken: string): Promise<Wire.Operation> {
    return this.client.request("POST", `${this.path}/operations/${segment(operationId)}/receipt`, receipt, { token: receiptToken });
  }
  retry(operationId: string, request: Wire.Retry): Promise<Wire.CommandReceipt> {
    return this.client.request("POST", `${this.path}/operations/${segment(operationId)}/retry`, request);
  }
  interactions(limit = 64, after = ""): Promise<Wire.Interaction[]> { return this.client.request("GET", `${this.path}/interactions?limit=${limit}&after=${segment(after)}`); }
  interact(request: Wire.InteractionCreate): Promise<Wire.Interaction> { return this.client.request("POST", `${this.path}/interactions`, request); }
  answer(interactionId: string, request: Wire.InteractionAnswer): Promise<Wire.Interaction> {
    return this.client.request("POST", `${this.path}/interactions/${segment(interactionId)}`, request);
  }
  connections(limit = 64, after = ""): Promise<Wire.Connection[]> { return this.client.request("GET", `${this.path}/connections?limit=${limit}&after=${segment(after)}`); }
  attach(template: string, options: { requestId?: string; resourceId?: string } = {}): Promise<Wire.Connection> {
    return this.client.request("POST", `${this.path}/connections`, { request_id: options.requestId ?? id(), template,
      ...(options.resourceId === undefined ? {} : { resource_id: options.resourceId }) });
  }
  revokeConnection(connectionId: string): Promise<Wire.Connection> { return this.client.request("DELETE", `${this.path}/connections/${segment(connectionId)}`); }
  documents(scope: "state" | "memory", limit = 64, after = ""): Promise<Wire.Document[]> { return this.client.request("GET", `${this.path}/${scope}?limit=${limit}&after=${segment(after)}`); }
  putDocument(scope: "state" | "memory", key: string, request: Wire.DocumentUpdate): Promise<Wire.Document> {
    return this.client.request("PUT", `${this.path}/${scope}/${segment(key)}`, request);
  }
  deleteDocument(scope: "state" | "memory", key: string): Promise<Wire.Document> { return this.client.request("DELETE", `${this.path}/${scope}/${segment(key)}`); }
  files(limit = 64, after = ""): Promise<Wire.FileInfo[]> { return this.client.request("GET", `${this.path}/files?limit=${limit}&after=${segment(after)}`); }
  putFile(name: string, body: Blob | Uint8Array, mediaType = "application/octet-stream"): Promise<Wire.FileInfo> {
    return this.client.request("PUT", `${this.path}/files/${segment(name)}`, body, { bytes: true, mediaType });
  }
  file(name: string): Promise<Uint8Array> { return this.client.request("GET", `${this.path}/files/${segment(name)}`, undefined, { bytes: true }); }
  deleteFile(name: string): Promise<Wire.FileInfo> { return this.client.request("DELETE", `${this.path}/files/${segment(name)}`); }
  schedules(limit = 64, after = ""): Promise<Wire.Schedule[]> { return this.client.request("GET", `${this.path}/schedules?limit=${limit}&after=${segment(after)}`); }
  schedule(request: Wire.ScheduleCreate): Promise<Wire.Schedule> { return this.client.request("POST", `${this.path}/schedules`, request); }
  cancelSchedule(scheduleId: string): Promise<Wire.Schedule> { return this.client.request("DELETE", `${this.path}/schedules/${segment(scheduleId)}`); }
  webhook(input: Wire.Input): Promise<Wire.CommandReceipt> { return this.client.request("POST", `${this.path}/webhooks`, input); }
  delegate(request: Wire.Delegation): Promise<Wire.SessionView> { return this.client.request("POST", `${this.path}/delegations`, request); }
  handoff(request: Wire.Handoff): Promise<Wire.SessionView> { return this.client.request("POST", `${this.path}/handoff`, request); }
  transferOwnership(request: Wire.TransferCreate): Promise<Wire.Transfer> {
    return this.client.request("POST", `/operator/sessions/${segment(this.id)}/transfers`, request);
  }
  ownershipTransfer(transferId: string): Promise<Wire.Transfer> {
    return this.client.request("GET", `/operator/sessions/${segment(this.id)}/transfers/${segment(transferId)}`);
  }
  cancelTransfer(transferId: string): Promise<Wire.Transfer> {
    return this.client.request("DELETE", `/operator/sessions/${segment(this.id)}/transfers/${segment(transferId)}`);
  }
  grant(request: Wire.ParticipantGrant): Promise<Wire.ParticipantToken> { return this.client.request("POST", `${this.path}/participants`, request); }
  revokeParticipant(participantId: string): Promise<string> { return this.client.request("DELETE", `${this.path}/participants/${segment(participantId)}`); }
  export(after = "0"): Promise<Wire.ExportManifest> { return this.client.request("GET", `${this.path}/export?after=${segment(after)}`); }
  delete(): Promise<Wire.SessionView> { return this.client.request("DELETE", this.path); }

  private async *items<T>(read: (after: string) => Promise<T[]>, key: (value: T) => string): AsyncGenerator<T> {
    let after = "";
    for (;;) {
      const page = await read(after);
      if (page.length === 0) return;
      const next = key(page[page.length - 1]!);
      if (next === after) throw new SessionError("corrupt", "Collection cursor did not advance", 0);
      yield* page;
      after = next;
    }
  }
  allRuns(): AsyncGenerator<Wire.Run> { return this.items(after => this.runs(64, after), value => value.id); }
  allOperations(): AsyncGenerator<Wire.Operation> { return this.items(after => this.operations(64, after), value => value.id); }
  allInteractions(): AsyncGenerator<Wire.Interaction> { return this.items(after => this.interactions(64, after), value => value.id); }
  allConnections(): AsyncGenerator<Wire.Connection> { return this.items(after => this.connections(64, after), value => value.id); }
  allDocuments(scope: "state" | "memory"): AsyncGenerator<Wire.Document> { return this.items(after => this.documents(scope, 64, after), value => value.key); }
  allFiles(): AsyncGenerator<Wire.FileInfo> { return this.items(after => this.files(64, after), value => value.name); }
  allSchedules(): AsyncGenerator<Wire.Schedule> { return this.items(after => this.schedules(64, after), value => value.id); }
  async *exportPages(): AsyncGenerator<Wire.ExportManifest> {
    let after = "0";
    for (;;) {
      const page = await this.export(after);
      yield page;
      if (!page.continuation) return;
      if (page.continuation === after) throw new SessionError("corrupt", "Export cursor did not advance", 0);
      after = page.continuation;
    }
  }

  async *events(options: { after?: string; signal?: AbortSignal } = {}): AsyncGenerator<StreamEvent> {
    const response = await this.client.fetch(`${this.client.url}${this.path}/events`, {
      headers: this.client.headers({ Accept: "text/event-stream", "Last-Event-ID": options.after ?? "0" }),
      redirect: "error", signal: options.signal,
    });
    if (!response.ok || response.body === null) throw new SessionError("unavailable", "Cannot open event stream", response.status);
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let pending = "";
    try {
      while (true) {
        const { value, done } = await reader.read();
        if (done) return;
        if (value.byteLength > 2097152) throw new SessionError("invalid", "SSE transport chunk exceeds two MiB", 0);
        pending = (pending + decoder.decode(value, { stream: true })).replace(/\r\n/gu, "\n");
        let boundary;
        while ((boundary = pending.indexOf("\n\n")) !== -1) {
          const frame = pending.slice(0, boundary);
          pending = pending.slice(boundary + 2);
          let event = ""; let cursor = ""; const data: string[] = [];
          for (const line of frame.split("\n")) {
            if (line.startsWith("event:")) event = line.slice(6).trimStart();
            else if (line.startsWith("id:")) cursor = line.slice(3).trimStart();
            else if (line.startsWith("data:")) data.push(line.slice(5).trimStart());
          }
          if (event !== "record" && event !== "gap") continue;
          if (!/^(0|[1-9][0-9]*)$/u.test(cursor)) throw new SessionError("invalid", "Invalid durable cursor", 0);
          const body = JSON.parse(data.join("\n"));
          yield event === "record" ? { kind: "record", id: cursor, record: body as Wire.Record }
            : { kind: "gap", id: cursor, gap: body as Wire.Gap };
        }
        if (new TextEncoder().encode(pending).length > 1048576) throw new SessionError("invalid", "SSE frame exceeds one MiB", 0);
      }
    } finally { await reader.cancel().catch(() => {}); reader.releaseLock(); }
  }

  async work(connection: Wire.Connection, handler: (execution: Wire.Execution) => Promise<Wire.Receipt>,
    receipts: ExecutionReceipts, options: { after?: string; signal?: AbortSignal } = {}): Promise<void> {
    for await (const stored of receipts.pending()) {
      if (stored.execution.operation.session_id !== this.id) continue;
      const receipt = stored.receipt ?? { request_id: `uncertain:${stored.execution.operation.id}`,
        outcome: "unknown" as const, error: "Application restarted after its claim; handler is not automatically repeated" };
      await this.receipt(stored.execution.operation.id, receipt, stored.execution.receipt_token);
      await receipts.acknowledged(stored.execution.operation.id);
    }
    const execute = async (operation: Wire.Operation) => {
      if (operation.recipient !== connection.id || operation.status !== "prepared") return;
      let execution: Wire.Execution;
      try {
        execution = await this.claim(operation.id, { request_id: id(), connection_id: connection.id, generation: connection.generation });
      } catch (error) { if (error instanceof SessionError && (error.status === 409 || error.status === 403)) return; throw error; }
      await receipts.begin(execution);
      let receipt: Wire.Receipt;
      try { receipt = await handler(execution); }
      catch { receipt = { request_id: id(), outcome: "unknown", error: "Application handler returned no authoritative outcome" }; }
      await receipts.complete(execution, receipt);
      await this.receipt(operation.id, receipt, execution.receipt_token);
      await receipts.acknowledged(operation.id);
    };
    for await (const operation of this.allOperations()) await execute(operation);
    for await (const event of this.events(options)) {
      if (event.kind !== "record") continue;
      if (event.record.kind === "interaction.answered") {
        for await (const operation of this.allOperations()) await execute(operation);
        continue;
      }
      if (event.record.kind !== "operation.prepared") continue;
      const operationId = (event.record.data as { id: string }).id;
      await execute(await this.operation(operationId));
    }
  }
}
