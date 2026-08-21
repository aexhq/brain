// The official aex agentloop, guest v0: the same policy as the in-process BuiltinAexLoop,
// driven over the engine-scoped ctx ops while the kernel manages context. Contract-scoped ops
// (model_stream and friends) replace the engine vocabulary when loop-owned context lands.
import { call } from "loophost:abi/host";

function ctx(op) {
  const response = JSON.parse(call(JSON.stringify({ op })));
  if (response.error) {
    // The host has already latched kernel errors; throwing surfaces guest-visible failure too.
    throw new Error(`${response.error.code}: ${response.error.message}`);
  }
  return response;
}

const verdict = (stop_reason, terminal_committed = false) =>
  JSON.stringify({ stop_reason, terminal_committed });

export function activate(kind, _payload) {
  if (kind !== "message") {
    return verdict("end_turn");
  }
  for (;;) {
    if (ctx("engine.prepare_round").outcome === "interrupted") {
      return verdict("interrupted");
    }
    const budget = ctx("engine.budget");
    if (budget.rounds >= budget.max_rounds) {
      return verdict("max_rounds");
    }
    const round = ctx("engine.model_round");
    if (round.outcome === "cancelled") return verdict("cancelled");
    if (round.outcome === "interrupted") return verdict("interrupted");
    if (round.outcome === "final") {
      return verdict(round.refusal ? "refusal" : "end_turn");
    }
    const dispatched = ctx("engine.dispatch_pending");
    if (dispatched.outcome === "terminal") {
      return JSON.stringify({
        stop_reason: dispatched.stop_reason,
        terminal_committed: true,
      });
    }
    if (ctx("engine.budget").cancelled) {
      return verdict("cancelled");
    }
  }
}
