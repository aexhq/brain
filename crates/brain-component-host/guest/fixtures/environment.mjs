export function resolve(request) {
  return { bindingJson: JSON.stringify({ environmentId: request.environmentId }) };
}

export function submit(_bindingJson, operation) {
  return { providerOperationId: `${operation.operationId}:${operation.bundle?.length ?? 0}` };
}

export function observe(_bindingJson, providerOperationId, cursor) {
  return {
    state: "completed",
    cursor: cursor ?? "terminal",
    chunksJson: "[]",
    terminalJson: JSON.stringify({ providerOperationId, value: "environment-ok" }),
  };
}

export function cancel() {}

export function acknowledge() {}

export function release() {}
