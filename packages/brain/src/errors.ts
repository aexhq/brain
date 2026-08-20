import type { ApiError, ApiErrorCode } from "./generated/session.js";

export interface BrainErrorOptions {
  code?: ApiErrorCode | undefined;
  status?: number | undefined;
  param?: string | undefined;
  requestId?: string | undefined;
  cause?: unknown;
}

export class BrainError extends Error {
  readonly code: ApiErrorCode | undefined;
  readonly status: number | undefined;
  readonly param: string | undefined;
  readonly requestId: string | undefined;

  constructor(message: string, options: BrainErrorOptions = {}) {
    super(message, options.cause === undefined ? undefined : { cause: options.cause });
    this.name = "BrainError";
    this.code = options.code;
    this.status = options.status;
    this.param = options.param;
    this.requestId = options.requestId;
  }
}

export class SessionError extends BrainError {
  constructor(message: string, options: BrainErrorOptions = {}) {
    super(message, options);
    this.name = "SessionError";
  }
}

export class AbortError extends BrainError {
  constructor(message = "The operation was cancelled", options: BrainErrorOptions = {}) {
    super(message, { ...options, code: "cancelled" });
    this.name = "AbortError";
  }
}

export function errorFromApi(
  error: ApiError,
  status?: number,
): BrainError {
  const options: BrainErrorOptions = {
    code: error.code,
    status,
    param: error.param,
    requestId: error.request_id,
  };
  switch (error.code) {
    case "cancelled":
      return new AbortError(error.message, options);
    default:
      return new SessionError(error.message, options);
  }
}

export function abortError(cause?: unknown): AbortError {
  return new AbortError("The operation was cancelled", { cause });
}
