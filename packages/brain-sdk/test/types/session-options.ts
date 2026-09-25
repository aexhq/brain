import { z } from "zod";
import type { SessionHandle, SessionState } from "../../dist/index.js";
// @ts-expect-error Typed-answer policy is not part of the Brain SDK.
import type { StructuredSendOptions } from "../../dist/index.js";
// @ts-expect-error Typed-answer policy is not part of the Brain SDK.
import { StructuredOutputError } from "../../dist/index.js";

declare const session: SessionHandle;
const ordinary: Promise<SessionState> = session.send("Chat");
const cancellable: Promise<SessionState> = session.send("Chat", { signal: new AbortController().signal });
// @ts-expect-error A raw Brain send has no typed-answer option.
session.send("Extract", { output: { type: z.string() } });
// @ts-expect-error Submit only admits a turn.
session.submit("Extract", { output: { type: z.string() } });
void [ordinary, cancellable];
