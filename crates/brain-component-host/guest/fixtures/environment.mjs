import { dispatch } from "aex:environment/host@1.0.0";

export function resolve(request) {
  const config = JSON.parse(request.configJson);
  return {
    bindingJson: JSON.stringify({
      environmentId: request.environmentId,
      dispatch: config.dispatch === true,
    }),
  };
}

export function submit(bindingJson, operation) {
  const suffix = `${operation.operationId}:${operation.bundle?.length ?? 0}`;
  if (JSON.parse(bindingJson).dispatch !== true) return { providerOperationId: suffix };
  const response = dispatch(
    operation.operationId,
    "submit",
    JSON.stringify({ operation_id: operation.operationId }),
    operation.deadlineAtMs,
  );
  return { providerOperationId: `${JSON.parse(response).dispatched}:${suffix}` };
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

export function release(bindingJson) {
  if (JSON.parse(bindingJson).dispatch !== true) return;
  dispatch("release", "release", JSON.stringify({ released: true }), 18446744073709551615n);
}
