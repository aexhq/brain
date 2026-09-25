import { environmentHandler, tool, type CreateSessionOptions } from "../../dist/index.js";
import { z } from "zod";

declare const required: Pick<CreateSessionOptions, "model" | "agentloop">;
const automatic: CreateSessionOptions = required;
const manual: CreateSessionOptions = { ...required, environment: { lifecycle: { default: "manual" } } };
const mixed: CreateSessionOptions = { ...required, environment: { lifecycle: { bindings: { workspace: "manual" } } } };
// @ts-expect-error Lifecycle policies use the supported values.
const invalid: CreateSessionOptions = { ...required, environment: { lifecycle: { default: "invalid" } } };
// @ts-expect-error Session environment options are grouped under environment.
const flat: CreateSessionOptions = { ...required, environmentLifecycle: { default: "automatic" } };
void [automatic, manual, mixed, invalid, flat];

const handler = environmentHandler({
  options: z.object({ region: z.string() }),
  methods: { inspect: {
    description: "Read one resource",
    input: z.object({ resource: z.string() }),
    run: (input, context) => {
      const resource: string = input.resource;
      const region: string = context.options.region;
      // @ts-expect-error Input follows the method's schema.
      input.missing;
      return { resource, region };
    },
  } },
  setup: async context => {
    const region: string = context.options.region;
    await context.environments.list();
    void region;
  },
});
void handler;
tool({ name: "custom", description: "Custom control", input: z.object({}), run: async (_, context) => {
  const environments = await context.environments.list();
  if (environments[0]) await context.environments.call(environments[0].reference, "inventory", {});
  await context.finish();
}})({ environments: [{ environment: "workspace", permissions: ["read", "call"], methods: ["inventory"] }] });
