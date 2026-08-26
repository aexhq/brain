import { invoke as invokeEnvironment } from "aex:tool/environment@1.0.0";

export function invoke(request) {
  const config = JSON.parse(request.configJson);
  if (config.useEnvironment === true) {
    const valueJson = invokeEnvironment(
      request.metadata.callId,
      "{}",
      request.inputJson,
      request.deadlineAtMs,
    );
    return { valueJson, content: valueJson, isError: false };
  }
  return {
    valueJson: request.inputJson,
    content: request.inputJson,
    isError: false,
  };
}
