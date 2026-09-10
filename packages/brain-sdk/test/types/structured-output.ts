import { z } from "zod";
import type { SessionHandle, SessionState, StructuredSendOptions } from "../../dist/index.js";

declare const session: SessionHandle;
const schema = z.object({ age: z.string().transform(Number) });
const options: StructuredSendOptions<typeof schema> = { output: { type: schema } };
const structured: Promise<{ age: number }> = session.send("Extract", options);
const ordinary: Promise<SessionState> = session.send("Chat");
const cancellable: Promise<SessionState> = session.send("Chat", { signal: new AbortController().signal });
// @ts-expect-error The return value is the transformed output, not the schema's input.
const input: Promise<{ age: string }> = session.send("Extract", options);
// @ts-expect-error Output needs a Zod type.
session.send("Extract", { output: { type: {} } });
// @ts-expect-error Retry counts are numeric.
session.send("Extract", { output: { type: schema, maxRetries: "2" } });
void [structured, ordinary, cancellable, input];
