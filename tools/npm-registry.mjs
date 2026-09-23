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
