import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { agentloop, component } from "@aexhq/brain";

const path = process.env.BRAIN_AGENTLOOP_WASM;
if (!path) throw new Error("BRAIN_AGENTLOOP_WASM must name a compiled Agentloop Component");

export const example = agentloop({
  implementation: component(pathToFileURL(resolve(path))),
});
