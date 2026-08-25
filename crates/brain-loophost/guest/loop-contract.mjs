// A contract-mode probe loop: drives its turn entirely through `contracts/agentloop/v1` ctx
// ops (loop-composed model requests, loop-directed dispatch, durable kv/journal state) and
// echoes what the kernel delivered back out as loop events so tests can assert on the public
// stream. External authoring SDKs generate this same contract vocabulary.
import { call } from "loophost:abi/host";

let opCounter = 0;
function op(activationId, opBody) {
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

function engineOp(name) {
  const response = JSON.parse(call(JSON.stringify({ op: name })));
  if (response.error) {
    throw new Error(`${response.error.code}: ${response.error.message}`);
  }
  return response;
}

// Loop memory is a cache that lives exactly as long as this resident instance: the delivered
// session_start hydration and the activation history prove residency semantics to the tests.
let deliveredHydration = null;
const activationKinds = [];

export function activate(kind, payload) {
  activationKinds.push(kind);
  if (kind === "session_start") {
    deliveredHydration = JSON.parse(payload);
    return JSON.stringify({
      activation_id: deliveredHydration.activation_id,
      outcome: "completed",
    });
  }
  if (kind !== "message") {
    return JSON.stringify({ activation_id: "act-none", outcome: "completed" });
  }
  const activation = JSON.parse(payload);
  const id = activation.activation_id;

  // Hydration probe: fetch the current payload each turn and publish what arrived, plus what
  // the host DELIVERED at instance start and every activation kind this instance has seen.
  const hydration = engineOp("engine.session_start");
  op(id, {
    op: "journal_append",
    entries: [{
      kind: "event",
      name: "loop.hydration",
      data: {
        resumed: hydration.resumed,
        kv: hydration.kv,
        tail_types: hydration.tail.map((entry) => entry.type),
        mark_covers: hydration.latest_mark ? hydration.latest_mark.covers_through_seq : null,
        mark_data: hydration.latest_mark ? hydration.latest_mark.data : null,
        activation_kinds: activationKinds.slice(),
        start_delivered: deliveredHydration !== null,
        start_resumed: deliveredHydration ? deliveredHydration.resumed : null,
      },
    }],
  });

  // Durable kv round-trip: read the committed counter, write the increment.
  const kv = op(id, { op: "kv_get", keys: ["turns"] });
  const turns = (kv.entries.turns || 0) + 1;
  op(id, { op: "kv_set", entries: { turns } });

  // Typed op errors are guest-visible and never kill the turn. An undeclared tool is not
  // an op error: dispatch answers it with a journaled failed result (never a route).
  let unsealed = null;
  let kvLimit = null;
  try {
    const dispatched = op(id, {
      op: "tools_dispatch",
      calls: [{ tool_call_id: "c1", name: "not_sealed", input: {} }],
    });
    const result = dispatched.results?.[0];
    unsealed = result && result.is_error ? "failed_result" : "executed";
  } catch (error) {
    unsealed = error.code;
  }
  try {
    op(id, { op: "kv_set", entries: { big: "x".repeat(9000) } });
  } catch (error) {
    kvLimit = error.code;
  }
  op(id, {
    op: "journal_append",
    entries: [{ kind: "event", name: "loop.checks", data: { unsealed, kv_limit: kvLimit } }],
  });

  // One model round composed by the loop from the activation's admitted message.
  const round = op(id, {
    op: "model_stream",
    request: {
      system: "you are the contract probe",
      messages: [{ role: "user", content: activation.message.content }],
    },
  });
  const calls = round.message.content.filter((block) => block.type === "tool_call");
  if (calls.length > 0) {
    const dispatched = op(id, {
      op: "tools_dispatch",
      calls: calls.map((block) => ({
        tool_call_id: block.tool_call_id,
        name: block.name,
        input: block.input,
      })),
    });
    op(id, {
      op: "journal_append",
      entries: [{
        kind: "event",
        name: "loop.dispatched",
        data: {
          results: dispatched.results.map((result) => ({
            name: result.name,
            is_error: result.is_error,
          })),
        },
      }],
    });
    // Continue with the results the way a real harness would.
    op(id, {
      op: "model_stream",
      request: {
        messages: [
          { role: "user", content: activation.message.content },
          { role: "assistant", content: round.message.content },
          ...dispatched.results.map((result) => ({
            role: "tool_result",
            tool_call_id: result.tool_call_id,
            name: result.name,
            is_error: result.is_error,
            content: result.content,
          })),
        ],
      },
    });
  }

  // Durable trail: a custom entry plus a mark covering through the admitted message, so the
  // next session_start tail starts at this turn's own records.
  op(id, {
    op: "journal_append",
    entries: [
      { kind: "custom", data: { note: `turn ${turns}` } },
      {
        kind: "mark",
        covers_through_seq: activation.message.seq,
        data: { summary: `through turn ${turns}` },
      },
    ],
  });
  op(id, { op: "turn_finish", result: { turns } });
  return JSON.stringify({ activation_id: id, outcome: "completed" });
}
