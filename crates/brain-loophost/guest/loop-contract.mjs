// A contract-mode probe loop: drives its turn entirely through `contracts/agentloop/v1` ctx
// ops (loop-composed model requests, loop-directed dispatch, durable kv/journal state) and
// echoes what the kernel delivered back out as loop events so tests can assert on the public
// stream. This is the shape the H2 loop SDK generates; the official aex loop stays engine-mode.
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

export function activate(kind, payload) {
  if (kind !== "message") {
    return JSON.stringify({ activation_id: "act-none", outcome: "completed" });
  }
  const activation = JSON.parse(payload);
  const id = activation.activation_id;

  // Hydration probe: fetch the session_start payload and publish what arrived.
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
      },
    }],
  });

  // Durable kv round-trip: read the committed counter, write the increment.
  const kv = op(id, { op: "kv_get", keys: ["turns"] });
  const turns = (kv.entries.turns || 0) + 1;
  op(id, { op: "kv_set", entries: { turns } });

  // Typed op errors are guest-visible and never kill the turn.
  let unsealed = null;
  let kvLimit = null;
  try {
    op(id, { op: "tools_dispatch", calls: [{ tool_call_id: "c1", name: "not_sealed", input: {} }] });
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
