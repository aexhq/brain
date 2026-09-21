export async function call(callback, method, input, fetch = globalThis.fetch) {
  if (!callback?.methods.includes(method)) throw new Error(`service ${method} is not granted`);
  const response = await fetch(callback.url, {
    method: "POST",
    headers: { authorization: `Bearer ${callback.token}`, "content-type": "application/json" },
    body: JSON.stringify({ method, input }),
  });
  if (!response.ok) throw new Error(`${method} answered ${response.status}`);
  return response.json();
}

export async function finish(callback, output, fetch) {
  try {
    await call(callback, "finish", { status: "ok", value: output }, fetch);
    return { type: "result", output };
  } catch (error) {
    return { type: "unknown", message: `Completion acknowledgement lost: ${error.message}` };
  }
}
