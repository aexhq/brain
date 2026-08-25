let response;

export function start(request) {
  const options = JSON.parse(request.providerOptionsJson);
  const messages = JSON.parse(request.messagesJson);
  const hasToolResult = messages.some((message) =>
    Array.isArray(message.content) && message.content.some((block) => block.type === "tool_result")
  );
  response = hasToolResult || options.toolName === undefined
    ? {
        state: "completed",
        events: [{ kind: "text-delta", payloadJson: JSON.stringify({ index: 0, text: options.finalText ?? "model-ok" }) }],
        terminalJson: JSON.stringify({ stopReason: "end_turn" }),
      }
    : {
        state: "completed",
        events: [
          { kind: "tool-use-start", payloadJson: JSON.stringify({ index: 0, id: "fixture-tool-call", name: options.toolName }) },
          { kind: "tool-input-delta", payloadJson: JSON.stringify({ index: 0, partialJson: JSON.stringify(options.toolInput ?? {}) }) },
        ],
        terminalJson: JSON.stringify({ stopReason: "tool_use" }),
      };
  return { providerOperationId: request.operationId };
}

export function observe(providerOperationId, cursor) {
  return {
    state: response.state,
    events: response.events.map((event) => ({ ...event, cursor: cursor ?? "1" })),
    nextCursor: undefined,
    terminalJson: response.terminalJson,
  };
}

export function cancel() {}

export function acknowledge() {}
