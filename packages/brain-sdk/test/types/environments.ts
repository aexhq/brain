import { environmentHandler, tool } from "../../dist/index.js";
import { z } from "zod";

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
