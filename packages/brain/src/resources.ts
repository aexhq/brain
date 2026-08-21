import type {
  Event,
  MessageAccepted,
  Session as SessionData,
  SessionList as SessionListData,
} from "./generated/session.js";
import type {
  FileEntry as CanonicalSandboxFileEntry,
  SandboxStatus as CanonicalSandboxStatus,
} from "./generated/hand.js";
import type { components as ApiComponents } from "./generated/paths.js";
import { randomIdempotencyKey } from "./json.js";
import type { EventOptions, JsonRequestOptions, SessionTransport } from "./transport.js";

function sid(sessionId: string): string {
  return encodeURIComponent(sessionId);
}

function childPath(sessionId: string, childId: string): string {
  return `/v1/sessions/${sid(sessionId)}/children/${encodeURIComponent(childId)}`;
}

export const MAX_INLINE_FILE_BYTES = 1024 * 1024;

type ApiSchemas = ApiComponents["schemas"];
export type TransferTicket = ApiSchemas["StorageTransfer"];

function assertInlineBase64(content: string): void {
  if (content.length > Math.ceil(MAX_INLINE_FILE_BYTES / 3) * 4 + 4) {
    throw new TypeError(`Inline file payload exceeds ${MAX_INLINE_FILE_BYTES} bytes; use a transfer ticket`);
  }
}

/** Exact canonical Hand lifecycle projection returned by the default-sandbox HTTP resource. */
export type SandboxStatus = CanonicalSandboxStatus;

/** Exact canonical Hand file metadata; timestamps are integer Unix milliseconds. */
export type SandboxFileEntry = CanonicalSandboxFileEntry;

export type SandboxFileList = ApiSchemas["SandboxFileList"];

export class SandboxFiles {
  constructor(readonly transport: SessionTransport, readonly sessionId: string) {}

  list(input: ApiSchemas["SandboxFileListRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileList> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/sandbox/files/list`, { ...options, body: input });
  }

  stat(input: ApiSchemas["SandboxFilePathRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileEntry> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/sandbox/files/stat`, { ...options, body: input });
  }

  readInline(input: ApiSchemas["SandboxFilePathRequest"], options: JsonRequestOptions = {}): Promise<ApiSchemas["SandboxFileContent"]> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/sandbox/files/read-inline`, { ...options, body: { ...input, max_bytes: MAX_INLINE_FILE_BYTES } });
  }

  writeInline(input: ApiSchemas["SandboxFileWriteRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileEntry> {
    assertInlineBase64(input.content_base64);
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/sandbox/files/write-inline`, { ...idempotent(options), body: input });
  }

  /** Happy-path convenience only; restart/expiry requires inspection and a fresh prepare. */
  prepareDownload(input: ApiSchemas["SandboxFilePathRequest"], options: JsonRequestOptions = {}): Promise<TransferTicket> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/sandbox/files/downloads`, { ...options, body: input });
  }

  /** Happy-path convenience only; restart/expiry/ambiguity requires inspection and a fresh prepare. */
  prepareUpload(input: ApiSchemas["SandboxFileUploadRequest"], options: JsonRequestOptions = {}): Promise<TransferTicket> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/sandbox/files/uploads`, { ...options, body: input });
  }

  /** Not auto-retried. Use storage plus copy when the transfer itself must be recovery-safe. */
  completeUpload(transferId: string, options: JsonRequestOptions = {}): Promise<SandboxFileEntry> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/sandbox/files/uploads/${encodeURIComponent(transferId)}/complete`, options);
  }

  find(input: ApiSchemas["SandboxFileFindRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileList> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/sandbox/files/find`, { ...options, body: input });
  }

  grep(input: ApiSchemas["SandboxFileGrepRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileList> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/sandbox/files/grep`, { ...options, body: input });
  }
}

export class SessionSandbox {
  readonly files: SandboxFiles;

  constructor(readonly transport: SessionTransport, readonly sessionId: string) {
    this.files = new SandboxFiles(transport, sessionId);
  }

  status(options: JsonRequestOptions = {}): Promise<SandboxStatus> {
    return this.transport.json("GET", `/v1/sessions/${sid(this.sessionId)}/sandbox`, options);
  }

  create(options: JsonRequestOptions = {}): Promise<SandboxStatus> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/sandbox`, options);
  }
}

export type StorageObject = ApiSchemas["StorageObject"];
export type StorageList = ApiSchemas["StorageList"];

export class SessionStorage {
  constructor(readonly transport: SessionTransport, readonly sessionId: string) {}

  list(input: ApiSchemas["StorageListRequest"] = {}, options: JsonRequestOptions = {}): Promise<StorageList> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/storage/list`, { ...options, body: input });
  }

  stat(key: string, options: JsonRequestOptions = {}): Promise<StorageObject> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/storage/stat`, { ...options, body: { key } });
  }

  readInline(key: string, options: JsonRequestOptions = {}): Promise<ApiSchemas["StorageContent"]> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/storage/read-inline`, { ...options, body: { key, max_bytes: MAX_INLINE_FILE_BYTES } });
  }

  writeInline(input: ApiSchemas["StorageWriteRequest"], options: JsonRequestOptions = {}): Promise<StorageObject> {
    assertInlineBase64(input.content_base64);
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/storage/write-inline`, { ...options, body: input });
  }

  prepareDownload(key: string, options: JsonRequestOptions = {}): Promise<TransferTicket> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/storage/downloads`, { ...options, body: { key } });
  }

  prepareUpload(input: ApiSchemas["StorageUploadRequest"], options: JsonRequestOptions = {}): Promise<TransferTicket> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/storage/uploads`, { ...options, body: input });
  }

  completeUpload(transferId: string, options: JsonRequestOptions = {}): Promise<StorageObject> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/storage/uploads/${encodeURIComponent(transferId)}/complete`, options);
  }

  delete(key: string, options: JsonRequestOptions = {}): Promise<void> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/storage/delete`, { ...options, body: { key } });
  }

  copyFromSandbox(input: ApiSchemas["StorageSandboxCopyRequest"], options: JsonRequestOptions = {}): Promise<StorageObject> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/storage/copy-from-sandbox`, { ...idempotent(options), body: input });
  }

  copyToSandbox(input: ApiSchemas["StorageSandboxCopyRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileEntry> {
    return this.transport.json("POST", `/v1/sessions/${sid(this.sessionId)}/storage/copy-to-sandbox`, { ...idempotent(options), body: input });
  }
}

/** Strongly consistent ordinary Session projections for the direct children of one Session. */
export type ChildList = SessionListData;

function idempotent(options: JsonRequestOptions): JsonRequestOptions {
  const headers = { ...options.headers };
  if (!Object.keys(headers).some((name) => name.toLowerCase() === "idempotency-key")) {
    headers["Idempotency-Key"] = randomIdempotencyKey();
  }
  return { ...options, headers, retry: true };
}

export class SessionChild {
  constructor(readonly transport: SessionTransport, readonly sessionId: string, readonly id: string) {}

  get(options: JsonRequestOptions = {}): Promise<SessionData> {
    return this.transport.json("GET", childPath(this.sessionId, this.id), options);
  }

  send(message: string, options: JsonRequestOptions = {}): Promise<MessageAccepted> {
    return this.transport.json("POST", `${childPath(this.sessionId, this.id)}/messages`, {
      ...idempotent(options),
      body: { message },
    });
  }

  followUp(message: string, options: JsonRequestOptions = {}): Promise<SessionData> {
    return this.transport.json("POST", `${childPath(this.sessionId, this.id)}/follow-up`, {
      ...idempotent(options),
      body: { message },
    });
  }

  wait(input: ApiSchemas["WaitChildRequest"] = {}, options: JsonRequestOptions = {}): Promise<SessionData> {
    return this.transport.json("POST", `${childPath(this.sessionId, this.id)}/wait`, { ...options, body: input });
  }

  interrupt(options: JsonRequestOptions = {}): Promise<SessionData> {
    return this.transport.json("POST", `${childPath(this.sessionId, this.id)}/interrupt`, options);
  }

  end(options: JsonRequestOptions = {}): Promise<SessionData> {
    return this.transport.json("POST", `${childPath(this.sessionId, this.id)}/end`, options);
  }

  events(options: EventOptions = {}): AsyncGenerator<Event> {
    return this.transport.events(this.id, options);
  }
}

export class SessionChildren {
  constructor(readonly transport: SessionTransport, readonly sessionId: string) {}

  create(input: { prompt: string; name?: string; fork_turns?: "all" | "none" | `${number}` }, options: JsonRequestOptions = {}): Promise<SessionChild> {
    return this.transport
      .json<SessionData>("POST", `/v1/sessions/${sid(this.sessionId)}/children`, {
        ...idempotent(options),
        body: input,
      })
      .then((child) => new SessionChild(this.transport, this.sessionId, child.id));
  }

  list(input: { cursor?: string; limit?: number } = {}, options: JsonRequestOptions = {}): Promise<ChildList> {
    const query = new URLSearchParams();
    if (input.cursor !== undefined) query.set("cursor", input.cursor);
    if (input.limit !== undefined) query.set("limit", String(input.limit));
    const suffix = query.size === 0 ? "" : `?${query}`;
    return this.transport.json("GET", `/v1/sessions/${sid(this.sessionId)}/children${suffix}`, options);
  }

  get(childId: string): SessionChild {
    return new SessionChild(this.transport, this.sessionId, childId);
  }
}
