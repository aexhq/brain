import isNumber from "is-number";
import { writeFile } from "node:fs/promises";

export default async function inspect(args: { value: string }, context: { operationId: string }) {
  let readOnly = false;
  try {
    await writeFile("/work/should-not-exist", "forbidden");
  } catch (error) {
    readOnly = error.code === "EROFS" || error.code === "EACCES";
  }
  return {
    dependency: isNumber(args.value),
    secret_injected: process.env.PACKAGE_SECRET === "journey-scoped-secret",
    internal_key_absent: process.env.BRAIN_WORKER_KEY === undefined,
    read_only: readOnly,
    uid: process.getuid(),
    operation_id: context.operationId,
  };
}
