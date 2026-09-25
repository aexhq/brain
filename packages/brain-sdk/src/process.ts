import { createInterface } from "node:readline";
import { runTool } from "./runtime.js";
import type { InvokeFrame } from "./host.js";

interface Invocation {
  implementation: unknown;
  input: unknown;
  sessionId: string;
  environment: string;
  sequence: number;
  deadline_at_ms?: number;
}

/** JSON-line service bridge for isolated processes; credentials stay in the Environment. */
export async function runToolProcess(directory = process.cwd()): Promise<void> {
  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  const pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();
  let next = 0;
  let open = true;
  const write = (value: unknown) => process.stdout.write(JSON.stringify(value) + "\n");
  const service = (method: string, input: unknown): Promise<unknown> => new Promise((resolve, reject) => {
    if (!open) { reject(new Error("Environment service connection closed")); return; }
    const id = ++next;
    pending.set(id, { resolve, reject });
    write({ type: "service", id, method, input });
  });
  const invocation = await new Promise<Invocation>((resolve, reject) => {
    lines.once("line", line => { try { resolve(JSON.parse(line)); } catch (error) { reject(error); } });
    lines.once("close", () => reject(new Error("Environment closed before invocation")));
  });
  lines.on("line", line => {
    try {
      const answer = JSON.parse(line);
      const call = pending.get(answer.id);
      if (!call) throw new Error("unknown Environment response");
      pending.delete(answer.id);
      if (answer.error !== undefined) call.reject(new Error(answer.error));
      else call.resolve(answer.output);
    } catch (error) {
      for (const call of pending.values()) call.reject(error as Error);
      pending.clear();
      lines.close();
    }
  });
  lines.on("close", () => {
    open = false;
    for (const call of pending.values()) call.reject(new Error("Environment service connection closed"));
    pending.clear();
  });
  // Ordinary console output must not corrupt invocation service frames.
  console.log = console.error;
  try {
    await runTool(invocation.implementation, directory, {
      sessionId: invocation.sessionId, environment: invocation.environment, sequence: invocation.sequence,
      arguments: invocation.input, ...(invocation.deadline_at_ms === undefined ? {} : { deadline_at_ms: invocation.deadline_at_ms }),
      emit: (kind, data) => service("emit", { event_type: kind, data }) as Promise<number>,
      update: value => service(value.type, value.outcome ?? null) as Promise<number>,
      model: request => service("model", request) as ReturnType<InvokeFrame["model"]>,
      environments: request => service("environments", request),
    });
    write({ type: "returned" });
  } catch (error) {
    write({ type: "failure", code: "tool_error", message: String((error as Error).message).slice(0, 4096), retryable: false });
  } finally {
    lines.close();
    process.stdin.destroy();
  }
}
