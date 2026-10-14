export function assertReleaseProvenance(payload, item) {
  const spec = `${item.name}@${item.version}`;
  const sha512 = Buffer.from(item.integrity.slice("sha512-".length), "base64").toString("hex");
  const purl = `pkg:npm/${item.name.replace(/^@/u, "%40")}@${item.version}`;
  if (
    payload.predicateType !== "https://slsa.dev/provenance/v1" ||
    !payload.subject?.some(({ name, digest }) => name === purl && digest?.sha512 === sha512)
  ) {
    throw new Error(`${spec} provenance does not identify the reviewed archive`);
  }
  const workflow = payload.predicate?.buildDefinition?.externalParameters?.workflow;
  const source = /^refs\/tags\/release\/sha-([0-9a-f]{40})$/u.exec(workflow?.ref ?? "")?.[1];
  const dependencies = payload.predicate?.buildDefinition?.resolvedDependencies ?? [];
  if (
    workflow?.repository !== "https://github.com/aexhq/brain" ||
    workflow?.path !== ".github/workflows/npm-publish.yml" ||
    source === undefined ||
    !dependencies.some(({ digest }) => digest?.gitCommit === source)
  ) {
    throw new Error(`${spec} provenance does not bind an immutable Brain release and workflow`);
  }
}

export function registryValue(run, spec, field) {
  let output;
  try {
    output = run(["view", spec, field, "--json", "--fetch-retries=0", "--fetch-timeout=10000"]);
  } catch (error) {
    let code;
    try { code = JSON.parse(String(error.stdout)).error?.code; } catch { throw error; }
    if (code === "E404") return undefined;
    throw error;
  }
  return output === "" ? undefined : JSON.parse(output);
}

export async function waitFor(read, expected, description) {
  const deadline = Date.now() + 300_000;
  while (Date.now() < deadline) {
    if (await read() === expected) return;
    await new Promise((resolve) => setTimeout(resolve, 5_000));
  }
  throw new Error(`${description} was not visible within 5 minutes; publication may have succeeded. Re-run the failed verification job using its original archives.`);
}
