// The official aex agentloop, contract mode: the default hosted policy driven entirely
// through `contracts/agentloop/v1` ctx ops, written with the public SDK exactly as a
// customer loop would be. The loop owns its conversation memory, durability marks and
// compaction; the kernel owns execution, journaling and recovery. The in-process twin is
// `crates/brain/src/agentloop.rs` BuiltinAexLoop — policy changes land in both.
//
// Policy:
//   - sealed presentation verbatim: no system/tools overrides, so the kernel reuses the
//     frozen provider base and prompt-cache key on every ordinary round;
//   - parallel dispatch of each round's whole tool batch;
//   - at the sealed round ceiling, one graceful closing round (tool_choice none) and a
//     max_rounds finish; refusal rounds finish as refusal;
//   - a summary mark per turn so rehydration never replays from zero;
//   - on budget_exceeded, loop-side compaction: summarize everything but a recent tail
//     through the sealed model and continue on summary + tail;
//   - provider failures fail the turn honestly through turn_fail (the kernel maps an
//     exhausted unknown-outcome budget to an interrupted turn on its own authority).
import { AgentloopOpError, defineAgentloop } from "@aexhq/agentloop";

const textOf = (content) =>
  content
    .filter((block) => block.type === "text")
    .map((block) => block.text)
    .join("");

const summaryMessage = (summary) => ({
  role: "user",
  content: [
    {
      type: "text",
      text: `<conversation_summary>\n${summary}\n</conversation_summary>`,
    },
  ],
});

function messageFromView(view) {
  switch (view.type) {
    case "user_message":
      return { role: "user", content: view.content };
    case "assistant_message":
      return { role: "assistant", content: view.message.content };
    case "tool_result":
      return {
        role: "tool_result",
        tool_call_id: view.result.tool_call_id,
        name: view.result.name,
        is_error: view.result.is_error,
        content: view.result.content,
      };
    default:
      return null;
  }
}

// Loop-owned conversation memory, cached for this resident instance and rebuilt from the
// delivered hydration (summary mark first, then the tail) on a fresh one.
let memory = [];

const writeMark = async (ctx, admitted, summary) => {
  await ctx.journal.append([
    {
      kind: "mark",
      covers_through_seq: admitted.seq,
      data: { summary: summary.slice(0, 4000) },
    },
  ]);
};

/** Loop-side compaction; returns the replacement memory or null when the conversation
 * cannot be compacted further. */
async function selfCompact(ctx, current) {
  const tailLen = Math.min(Math.max(current.length - 2, 0), 4);
  if (tailLen === 0 || current.length < tailLen + 2) {
    return null;
  }
  const tail = current.slice(current.length - tailLen);
  let head = current.slice(0, current.length - tailLen);
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      const round = await ctx.model.stream({
        system: "You compact agent conversations. Reply with the summary text only.",
        messages: [
          ...head,
          {
            role: "user",
            content: [
              {
                type: "text",
                text: "Summarize the conversation above for a successor agent: goals, constraints, decisions, tool outcomes, unresolved failures, identifiers and next actions. Plain text.",
              },
            ],
          },
        ],
      });
      return [summaryMessage(textOf(round.content)), ...tail];
    } catch (error) {
      if (
        error instanceof AgentloopOpError &&
        error.code === "budget_exceeded" &&
        head.length > 2
      ) {
        head = head.slice(Math.floor(head.length / 2));
        continue;
      }
      throw error;
    }
  }
  return null;
}

export const { activate } = defineAgentloop({
  onSessionStart(start) {
    memory = [];
    // The sealed fork prefix, already in the loop's own message shape, precedes everything.
    for (const message of start.inherited ?? []) {
      memory.push(message);
    }
    if (start.latest_mark && typeof start.latest_mark.data.summary === "string") {
      memory.push(summaryMessage(start.latest_mark.data.summary));
    }
    for (const view of start.tail) {
      const message = messageFromView(view);
      if (message !== null) {
        memory.push(message);
      }
    }
  },
  async onMessage(ctx, admitted) {
    memory.push({ role: "user", content: admitted.content });
    const maxRounds = ctx.session.limits.max_rounds_per_turn;
    let rounds = 0;
    try {
      for (;;) {
        const closing = rounds >= maxRounds;
        const request = { messages: memory.slice() };
        if (closing) {
          request.tool_choice = "none";
        }
        let round;
        try {
          round = await ctx.model.stream(request);
        } catch (error) {
          if (
            error instanceof AgentloopOpError &&
            error.code === "budget_exceeded" &&
            !closing
          ) {
            const compacted = await selfCompact(ctx, memory);
            if (compacted === null) {
              await ctx.turn.fail(error);
              return;
            }
            memory = compacted;
            continue;
          }
          if (error instanceof AgentloopOpError && error.code === "provider_error") {
            await ctx.turn.fail(error);
            return;
          }
          throw error;
        }
        rounds += 1;
        memory.push({ role: "assistant", content: round.content });
        if (closing) {
          await writeMark(ctx, admitted, textOf(round.content));
          await ctx.turn.finish(undefined, { stopReason: "max_rounds" });
          return;
        }
        if (round.stop_reason === "refusal") {
          await writeMark(ctx, admitted, textOf(round.content));
          await ctx.turn.finish(undefined, { stopReason: "refusal" });
          return;
        }
        const calls = round.content.filter((block) => block.type === "tool_call");
        if (calls.length === 0) {
          await writeMark(ctx, admitted, textOf(round.content));
          await ctx.turn.finish(undefined, { stopReason: "end_turn" });
          return;
        }
        // aex policy: the whole batch dispatches in parallel.
        const results = await ctx.tools.dispatch(
          calls.map((call) => ({
            tool_call_id: call.tool_call_id,
            name: call.name,
            input: call.input,
          })),
        );
        for (const result of results) {
          memory.push({
            role: "tool_result",
            tool_call_id: result.tool_call_id,
            name: result.name,
            is_error: result.is_error,
            content: result.content,
          });
        }
      }
    } catch (error) {
      // A return_direct tool committed the turn terminal mid-loop: every further ctx op
      // fails turn_already_terminal, and the clean exit is simply to stop driving.
      if (error instanceof AgentloopOpError && error.code === "turn_already_terminal") {
        return;
      }
      throw error;
    }
  },
});
