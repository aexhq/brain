// A customer-shaped probe that tries to drive the kernel's engine vocabulary. The e2e
// uploads this bundle through the public path and asserts the host refuses every engine
// round op for contract-only guests — the enforcement behind the wit doc's claim.
import { call } from "loophost:abi/host";

let opCounter = 0;
function contractOp(activationId, opBody) {
  const request = {
    op_id: `op-${++opCounter}`,
    activation_id: activationId,
    op: opBody,
  };
  const response = JSON.parse(call(JSON.stringify(request)));
  if (response.error) {
    const error = new Error(response.error.message);
    error.code = response.error.code;
    throw error;
  }
  return response.result;
}

export function activate(kind, payload) {
  const activation = JSON.parse(payload);
  if (kind !== "message") {
    return JSON.stringify({ activation_id: activation.activation_id, outcome: "completed" });
  }
  const id = activation.activation_id;
  // Probe the reserved vocabulary; record exactly what the host answered.
  const refusals = {};
  for (const op of ["engine.model_round", "engine.dispatch_pending", "engine.budget"]) {
    const response = JSON.parse(call(JSON.stringify({ op })));
    refusals[op] = response.error
      ? { code: response.error.code, message: response.error.message }
      : { served: response };
  }
  // The open exception: read-only residency hydration stays reachable.
  const hydration = JSON.parse(call(JSON.stringify({ op: "engine.session_start" })));
  contractOp(id, {
    op: "turn_finish",
    result: { refusals, session_start_served: hydration.error === undefined },
  });
  return JSON.stringify({ activation_id: id, outcome: "completed" });
}
