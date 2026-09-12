import { Brain, type SessionHandle } from "../../dist/index.js";

const client = new Brain({ baseUrl: "https://brain.example" });
declare const session: SessionHandle;
const closing: Promise<void> = client.close();
const interrupting: Promise<void> = session.interrupt({ idempotencyKey: "interrupt-once" });
void closing;
void interrupting;
// @ts-expect-error The public interruption method has one spelling.
session.cancel();
