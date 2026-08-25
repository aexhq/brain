export function start(request) {
  return { providerOperationId: request.operationId };
}

export function observe(providerOperationId, cursor) {
  return {
    state: "completed",
    events: [
      {
        cursor: cursor ?? "1",
        kind: "text-delta",
        payloadJson: JSON.stringify({ text: "model-ok", providerOperationId }),
      },
    ],
    nextCursor: undefined,
    terminalJson: JSON.stringify({ stopReason: "end_turn" }),
  };
}

export function cancel() {}

export function acknowledge() {}
