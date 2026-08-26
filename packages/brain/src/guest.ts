/**
 * Authoring helpers for code running inside a component, beside the contracts it implements.
 *
 * A component export declares `result<_, extension-error>`, but componentize-js rethrows a plain
 * `Error` as an opaque wasm trap that reaches the kernel without a reason. Every export reports
 * through this guard so a host or marshalling failure keeps its message. A host import already
 * throws its own typed payload; that one is preserved exactly. `kind` names the component the
 * failure belongs to, so a Tool export does not report an Environment code.
 */
export function typed<Value>(operation: string, call: () => Value, kind: string): Value {
  try {
    return call();
  } catch (error) {
    if (
      error !== null &&
      typeof error === "object" &&
      Object.prototype.hasOwnProperty.call(error, "payload")
    ) {
      throw error;
    }
    throw {
      payload: {
        code: `${kind}_${operation}_failed`,
        message: String((error as { message?: unknown } | null)?.message ?? error).slice(0, 4096),
        retryable: false,
      },
    };
  }
}
