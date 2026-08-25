export function invoke(request) {
  return {
    valueJson: request.inputJson,
    content: request.inputJson,
    isError: false,
  };
}
