// Only deployment-selected origins supply image bytes to native model inputs.
export async function loadImages(media = [], origins = [], signal) {
  let size = 0;
  const images = [];
  for (const input of media) {
    const source = new URL(input.url);
    const origin = origins.find(origin => new URL(origin).origin === source.origin);
    if (input.type !== "image" || !origin || source.username || source.password) throw new Error("image origin denied");
    const target = new URL(origin);
    target.pathname = source.pathname; target.search = source.search;
    const response = await fetch(target, { redirect: "error", signal: AbortSignal.any([AbortSignal.timeout(15000), ...(signal ? [signal] : [])]) });
    if (!response.ok || !response.body) throw new Error("image unavailable");
    const mimeType = response.headers.get("content-type")?.split(";", 1)[0];
    if (!["image/png", "image/jpeg", "image/webp", "image/gif"].includes(mimeType)) { await response.body.cancel(); throw new Error("unsupported image format"); }
    const chunks = [];
    for await (const chunk of response.body) {
      size += chunk.byteLength;
      if (size > 20 << 20) throw new Error("image inputs exceed 20 MiB");
      chunks.push(chunk);
    }
    images.push({ type: "image", mimeType, data: Buffer.concat(chunks).toString("base64") });
  }
  return images;
}
