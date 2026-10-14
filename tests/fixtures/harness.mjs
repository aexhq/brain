// Deployment-owned diagnostic harness. Scenario selection is test data, never code.
import assert from "node:assert/strict";
import { serveHarness } from "@aexhq/brain/harness";
import { piHarness } from "@aexhq/brain/harness/pi";

const pi = piHarness();
await serveHarness({
  name: "brain-test", stateVersion: "1",
  async open(options) {
    const native = await pi.open({ ...options, configuration: { contextWindow: 64000, maxTokens: 1024, compaction: { enabled: false } } });
    return {
      async run(ctx) {
        const { mode = "pi", ...configuration } = ctx.configuration ?? {};
        if (mode === "pi") return native.run({ ...ctx, configuration });
        if (mode === "diagnostic" || mode === "kv-fail") {
          await ctx.readEvents(0);
          const turns = (await ctx.kv.get("memory"))?.turns + 1 || 1;
          await ctx.kv.set("memory", { turns });
          await ctx.emit("note", { turns });
          if (ctx.input?.message === "kv" || mode === "kv-fail") {
            assert.equal(await ctx.kv.get("deleted"), undefined);
            await ctx.kv.set("deleted", null);
            assert.equal(await ctx.kv.get("deleted"), null);
            await ctx.kv.delete("deleted");
            assert.equal(await ctx.kv.get("deleted"), undefined);
            await ctx.kv.delete("deleted");
          }
          if (mode === "kv-fail") throw new Error("failed after KV commits");
          return { turns, message: ctx.input?.message ?? "" };
        }
        if (mode === "observer") {
          if (ctx.input) {
            const environment = ctx.tools.find(tool => tool.name === "work").environments[0];
            const returned = await ctx.callTool({ call_id: "background", environment, name: "work", input: { background: true } });
            await ctx.emit("tool_return", returned);
            await ctx.setTranscript([{ role: "user", content: [{ type: "text", text: "harness-selected" }] }]);
          }
          let after = await ctx.kv.get("observer_cursor") ?? 0;
          let followed = await ctx.kv.get("followed");
          for (;;) {
            const page = await ctx.readEvents(after);
            if (!page.events.length) break;
            for (const event of page.events) if (event.event_type === "tool_call_ended") {
              const followup = ctx.tools.find(tool => tool.name === "followup");
              if (followup && !followed) {
                followed = true; await ctx.kv.set("followed", true);
                await ctx.callTool({ call_id: "followup", environment: followup.environments[0], name: "followup", input: {} });
              }
              await ctx.kv.set("observed_finish", event.sequence);
            }
            after = page.next_cursor;
          }
          await ctx.kv.set("observer_cursor", after);
          return;
        }
        if (mode === "selection") {
          const [definition] = ctx.tools;
          const transcript = [{ role: "user", content: [{ type: "text", text: "choose a placement" }] }];
          const model = await ctx.model({ messages: transcript, tools: [{
            name: definition.name, description: definition.description,
            input_schema: { type: "object", properties: { environment: { type: "string", enum: definition.environments }, input: definition.input_schema },
              required: ["environment", "input"], additionalProperties: false },
          }] });
          transcript.push(model.message);
          const returned = await ctx.callTools(model.message.content.filter(block => block.type === "tool_use").map(block => ({
            call_id: block.id, name: block.name, environment: block.input.environment, input: block.input.input,
          })));
          const results = returned.map(value => value.events.find(event => event.event_type === "tool_result_emitted").data.result);
          transcript.push({ role: "user", content: results.map(result => ({ type: "tool_result", tool_use_id: result.call_id, content: result.output, is_error: result.is_error })) });
          await ctx.setTranscript(transcript); await ctx.readEvents(0);
          return results[0].output;
        }
        throw new Error(`unknown test scenario: ${mode}`);
      },
      cancel: () => native.cancel(), close: () => native.close(),
    };
  },
});
