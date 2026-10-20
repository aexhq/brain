import type { Brain, SessionInput } from "./index.js";
import type { RuntimeTool } from "./runtime.js";
import type { Session } from "./generated.js";

/** Call directly from a trusted click handler. The page owns its executor. */
export async function connectTab(brain: Brain, input: { create: SessionInput } | { session: string; tools: RuntimeTool[] }): Promise<{ session: Session; detach(): Promise<void> }> {
  if (typeof window === "undefined" || !navigator.userActivation.isActive) throw new Error("attach the tab from a user gesture");
  const lifetime = new AbortController();
  const unload = () => { lifetime.abort(); void brain.dispose().catch(() => {}); };
  window.addEventListener("pagehide", unload, { once: true });
  const detach = async () => { window.removeEventListener("pagehide", unload); lifetime.abort(); await brain.dispose(); };
  try {
    let session: Session;
    if ("create" in input) session = await brain.create(input.create, lifetime.signal);
    else { session = await brain.get(input.session, lifetime.signal); await brain.rebind(input.session, input.tools, "sse"); }
    if (lifetime.signal.aborted) throw new Error("the attached tab closed");
    return { session, detach };
  } catch (error) { await detach(); throw error; }
}
