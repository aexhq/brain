let activations = 0;

export function activate(request) {
  if (JSON.parse(request.configJson).track === true) {
    activations += 1;
    return { payloadJson: JSON.stringify({ activations }) };
  }
  return { payloadJson: request.payloadJson };
}
