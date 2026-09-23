import { tool } from "../../dist/index.js";
import { z } from "zod";

const plain = tool({ name: "plain", description: "Plain", input: z.object({}), run: (_, ctx) => ctx.finish(1) });
plain();
plain({});
const configured = tool({ name: "configured", description: "Configured", input: z.object({}), options: z.object({ prefix: z.string() }), run: (_, ctx) => ctx.finish(ctx.options.prefix) });
configured({ prefix: "ok" });
// @ts-expect-error Required configuration stays required with default placement.
configured();
// @ts-expect-error Missing required configuration.
configured({});
