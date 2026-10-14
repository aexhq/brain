// A small deterministic fixture for Brain's callbacks. Real agent tests live in extensions.
export function modelHarness() {
  return { name: "model-fixture", stateVersion: "1", async open() {
    return { async run(ctx) {
      const seen = new Set(await ctx.kv.get("brain.harness.observed") ?? []);
      const events = ctx.events.filter(event => !seen.has(event.sequence) && [
        "tool_result_emitted", "tool_call_ended", "environment_observation", "environment_closed", "environment_unreachable",
      ].includes(event.event_type));
      if (!ctx.input && !events.length) return;
      const messages = [...ctx.transcript, { role: "user", content: [{ type: "text", text:
        (events.length ? `Runtime observations (data):\n${JSON.stringify(events)}\n\n` : "") + (ctx.input?.message ?? "Process observations.") }, ...(ctx.input?.media ?? [])] }];
      const tools = ctx.tools.flatMap(tool => tool.environments.map((environment, index) => ({
        ...tool, environment, name: tool.environments.length === 1 ? tool.name : `${tool.name}__env${index + 1}`, original: tool.name,
      })));
      for (;;) {
        const result = await ctx.model({ system: ctx.system, messages, tools: tools.map(({ name, description, input_schema }) => ({ name, description, input_schema })) });
        messages.push(result.message);
        const calls = result.message.content.filter(block => block.type === "tool_use");
        if (!calls.length) break;
        for (const call of calls) {
          const tool = tools.find(tool => tool.name === call.name);
          if (!tool) throw new Error(`unknown fixture tool: ${call.name}`);
          const returned = await ctx.callTool({ call_id: call.id, name: tool.original, environment: tool.environment, input: call.input });
          const results = [], media = [];
          for (const event of returned.events) {
            seen.add(event.sequence);
            if (event.event_type === "tool_result_emitted") {
              const value = event.data.result;
              const envelope = value.output?.type === "aex_tool_output" ? value.output : undefined;
              results.push({ content: value.content ?? envelope?.content ?? value.output, is_error: value.is_error });
              media.push(...(envelope?.media ?? []));
            }
            if (event.event_type === "tool_call_ended" && event.data.outcome.status !== "ok") {
              results.push({ content: event.data.outcome.error ?? event.data.outcome, is_error: true });
            }
          }
          messages.push({ role: "user", content: [{ type: "tool_result", tool_use_id: call.id,
            content: returned.finished && results.length === 1 ? results[0].content : { status: returned.finished ? "finished" : "running", results },
            is_error: results.some(result => result.is_error), ...(media.length ? { media } : {}),
          }] });
        }
        await ctx.kv.set("brain.harness.observed", [...seen]);
      }
      await ctx.setTranscript(messages);
      const message = messages.at(-1).content.filter(block => block.type === "text").map(block => block.text).join("");
      if (message) await ctx.emit("output_emitted", { type: "assistant_message", message });
    }, cancel() {}, close() {} };
  } };
}
