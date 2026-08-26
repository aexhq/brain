import type {
  Event,
  MessageAccepted,
  Session as SessionData,
  SessionList as SessionListData,
} from "./generated/session.js";
import type {
  FileEntry as CanonicalSandboxFileEntry,
  SandboxStatus as CanonicalSandboxStatus,
} from "./generated/environment.js";
import type { components as ApiComponents, paths as ApiPaths } from "./generated/paths.js";
import { randomIdempotencyKey } from "./json.js";
import type { EventOptions, JsonRequestOptions, SessionTransport } from "./transport.js";

/** Builds one request URL from a path the Brain-owned OpenAPI declares. A literal that is not a
 *  declared path, or a missing parameter, fails to compile rather than reaching Brain as a 404. */
function path<P extends keyof ApiPaths & string>(
  template: P,
  params: Readonly<Record<string, string>>,
): string {
  return template.replace(/\{(\w+)\}/gu, (_match, name: string) => {
    const value = params[name];
    if (value === undefined) throw new TypeError(`${template} is missing ${name}`);
    return encodeURIComponent(value);
  });
}

export const MAX_INLINE_FILE_BYTES = 1024 * 1024;

type ApiSchemas = ApiComponents["schemas"];
export type TransferTicket = ApiSchemas["StorageTransfer"];

function assertInlineBase64(content: string): void {
  if (content.length > Math.ceil(MAX_INLINE_FILE_BYTES / 3) * 4 + 4) {
    throw new TypeError(`Inline file payload exceeds ${MAX_INLINE_FILE_BYTES} bytes; use a transfer ticket`);
  }
}

/** Exact canonical lifecycle projection returned by an environment HTTP resource. */
export type SandboxStatus = CanonicalSandboxStatus;

/** Exact canonical Environment file metadata; timestamps are integer Unix milliseconds. */
export type SandboxFileEntry = CanonicalSandboxFileEntry;

export type SandboxFileList = ApiSchemas["SandboxFileList"];

export class SandboxFiles {
  readonly #route: Readonly<Record<string, string>>;

  constructor(
    readonly transport: SessionTransport,
    readonly sessionId: string,
    readonly environment: string,
  ) {
    this.#route = { session_id: sessionId, environment };
  }

  list(input: ApiSchemas["SandboxFileListRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileList> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/environments/{environment}/files/list", this.#route), { ...options, body: input });
  }

  stat(input: ApiSchemas["SandboxFilePathRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileEntry> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/environments/{environment}/files/stat", this.#route), { ...options, body: input });
  }

  readInline(input: ApiSchemas["SandboxFilePathRequest"], options: JsonRequestOptions = {}): Promise<ApiSchemas["SandboxFileContent"]> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/environments/{environment}/files/read-inline", this.#route), { ...options, body: { ...input, max_bytes: MAX_INLINE_FILE_BYTES } });
  }

  writeInline(input: ApiSchemas["SandboxFileWriteRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileEntry> {
    assertInlineBase64(input.content_base64);
    return this.transport.json("POST", path("/v1/sessions/{session_id}/environments/{environment}/files/write-inline", this.#route), { ...idempotent(options), body: input });
  }

  /** Happy-path convenience only; restart/expiry requires inspection and a fresh prepare. */
  prepareDownload(input: ApiSchemas["SandboxFilePathRequest"], options: JsonRequestOptions = {}): Promise<TransferTicket> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/environments/{environment}/files/downloads", this.#route), { ...options, body: input });
  }

  /** Happy-path convenience only; restart/expiry/ambiguity requires inspection and a fresh prepare. */
  prepareUpload(input: ApiSchemas["SandboxFileUploadRequest"], options: JsonRequestOptions = {}): Promise<TransferTicket> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/environments/{environment}/files/uploads", this.#route), { ...options, body: input });
  }

  /** Not auto-retried. Use storage plus copy when the transfer itself must be recovery-safe. */
  completeUpload(transferId: string, options: JsonRequestOptions = {}): Promise<SandboxFileEntry> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/environments/{environment}/files/uploads/{transfer_id}/complete", { ...this.#route, transfer_id: transferId }), options);
  }

  find(input: ApiSchemas["SandboxFileFindRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileList> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/environments/{environment}/files/find", this.#route), { ...options, body: input });
  }

  grep(input: ApiSchemas["SandboxFileGrepRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileList> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/environments/{environment}/files/grep", this.#route), { ...options, body: input });
  }
}

/** One Environment the session declared, addressed by its declared name. Brain has no unnamed
 *  default: `Session.environments` lists the names this session can address. */
export class SessionSandbox {
  readonly files: SandboxFiles;
  readonly #route: Readonly<Record<string, string>>;

  constructor(
    readonly transport: SessionTransport,
    readonly sessionId: string,
    readonly environment: string,
  ) {
    this.files = new SandboxFiles(transport, sessionId, environment);
    this.#route = { session_id: sessionId, environment };
  }

  status(options: JsonRequestOptions = {}): Promise<SandboxStatus> {
    return this.transport.json("GET", path("/v1/sessions/{session_id}/environments/{environment}", this.#route), options);
  }

  create(options: JsonRequestOptions = {}): Promise<SandboxStatus> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/environments/{environment}", this.#route), options);
  }
}

export type StorageObject = ApiSchemas["StorageObject"];
export type StorageList = ApiSchemas["StorageList"];

export class SessionStorage {
  readonly #route: Readonly<Record<string, string>>;

  constructor(readonly transport: SessionTransport, readonly sessionId: string) {
    this.#route = { session_id: sessionId };
  }

  list(input: ApiSchemas["StorageListRequest"] = {}, options: JsonRequestOptions = {}): Promise<StorageList> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/storage/list", this.#route), { ...options, body: input });
  }

  stat(key: string, options: JsonRequestOptions = {}): Promise<StorageObject> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/storage/stat", this.#route), { ...options, body: { key } });
  }

  readInline(key: string, options: JsonRequestOptions = {}): Promise<ApiSchemas["StorageContent"]> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/storage/read-inline", this.#route), { ...options, body: { key, max_bytes: MAX_INLINE_FILE_BYTES } });
  }

  writeInline(input: ApiSchemas["StorageWriteRequest"], options: JsonRequestOptions = {}): Promise<StorageObject> {
    assertInlineBase64(input.content_base64);
    return this.transport.json("POST", path("/v1/sessions/{session_id}/storage/write-inline", this.#route), { ...options, body: input });
  }

  prepareDownload(key: string, options: JsonRequestOptions = {}): Promise<TransferTicket> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/storage/downloads", this.#route), { ...options, body: { key } });
  }

  prepareUpload(input: ApiSchemas["StorageUploadRequest"], options: JsonRequestOptions = {}): Promise<TransferTicket> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/storage/uploads", this.#route), { ...options, body: input });
  }

  completeUpload(transferId: string, options: JsonRequestOptions = {}): Promise<StorageObject> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/storage/uploads/{transfer_id}/complete", { ...this.#route, transfer_id: transferId }), options);
  }

  delete(key: string, options: JsonRequestOptions = {}): Promise<void> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/storage/delete", this.#route), { ...options, body: { key } });
  }

  copyFromEnvironment(environment: string, input: ApiSchemas["StorageEnvironmentCopyRequest"], options: JsonRequestOptions = {}): Promise<StorageObject> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/storage/copy-from-environment/{environment}", { ...this.#route, environment }), { ...idempotent(options), body: input });
  }

  copyToEnvironment(environment: string, input: ApiSchemas["StorageEnvironmentCopyRequest"], options: JsonRequestOptions = {}): Promise<SandboxFileEntry> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/storage/copy-to-environment/{environment}", { ...this.#route, environment }), { ...idempotent(options), body: input });
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
  readonly #route: Readonly<Record<string, string>>;

  constructor(readonly transport: SessionTransport, readonly sessionId: string, readonly id: string) {
    this.#route = { session_id: sessionId, child_id: id };
  }

  get(options: JsonRequestOptions = {}): Promise<SessionData> {
    return this.transport.json("GET", path("/v1/sessions/{session_id}/children/{child_id}", this.#route), options);
  }

  send(message: string, options: JsonRequestOptions = {}): Promise<MessageAccepted> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/children/{child_id}/messages", this.#route), {
      ...idempotent(options),
      body: { message },
    });
  }

  followUp(message: string, options: JsonRequestOptions = {}): Promise<SessionData> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/children/{child_id}/follow-up", this.#route), {
      ...idempotent(options),
      body: { message },
    });
  }

  wait(input: ApiSchemas["WaitChildRequest"] = {}, options: JsonRequestOptions = {}): Promise<SessionData> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/children/{child_id}/wait", this.#route), { ...options, body: input });
  }

  interrupt(options: JsonRequestOptions = {}): Promise<SessionData> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/children/{child_id}/interrupt", this.#route), options);
  }

  end(options: JsonRequestOptions = {}): Promise<SessionData> {
    return this.transport.json("POST", path("/v1/sessions/{session_id}/children/{child_id}/end", this.#route), options);
  }

  events(options: EventOptions = {}): AsyncGenerator<Event> {
    return this.transport.events(this.id, options);
  }
}

export class SessionChildren {
  readonly #route: Readonly<Record<string, string>>;

  constructor(readonly transport: SessionTransport, readonly sessionId: string) {
    this.#route = { session_id: sessionId };
  }

  create(input: { prompt: string; name?: string; fork_turns?: "all" | "none" | `${number}` }, options: JsonRequestOptions = {}): Promise<SessionChild> {
    return this.transport
      .json<SessionData>("POST", path("/v1/sessions/{session_id}/children", this.#route), {
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
    return this.transport.json("GET", path("/v1/sessions/{session_id}/children", this.#route) + suffix, options);
  }

  get(childId: string): SessionChild {
    return new SessionChild(this.transport, this.sessionId, childId);
  }
}
