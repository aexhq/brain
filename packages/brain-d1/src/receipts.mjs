import { big, digest, need, raw, wire } from "./domain.mjs";

export const receiptMetadataBytes = 48 * 1024;
export const outputReservation = limit => (limit + receiptMetadataBytes) * 3 + 4096;
const bytes = value => new TextEncoder().encode(value).byteLength;

export async function boundedReceipt(receipt, resultText, providerText, limit) {
  need(/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u.test(receipt.request_id), "invalid",
    "receipt request identifier exceeds its admitted bound");
  if (receipt.error !== undefined && bytes(receipt.error) > 1024) {
    let prefix = "", count = 0;
    for (const character of receipt.error) {
      count += bytes(character);
      if (count > 1024) break;
      prefix += character;
    }
    receipt.error = prefix + " [truncated]";
  }
  if (receipt.usage !== undefined) {
    const usage = { ...receipt.usage, provider: receipt.usage.provider ?? {} };
    receipt.usage = usage;
    const provider = providerText && providerText !== "null" ? providerText : "{}";
    if (bytes(provider) > 32 * 1024) {
      usage.provider = {};
      usage.provider_unavailable = { digest: await digest(provider), bytes: bytes(provider) };
    }
    need(!usage.provider_unavailable || (/^[a-f0-9]{64}$/u.test(usage.provider_unavailable.digest) &&
      big(usage.provider_unavailable.bytes) >= 0n && big(usage.provider_unavailable.bytes) <= 9007199254740991n),
    "invalid", "unavailable usage evidence exceeds its admitted bound");
  }
  const metadata = { ...receipt };
  delete metadata.result;
  need(bytes(wire("Receipt", metadata)) <= receiptMetadataBytes, "invalid", "receipt metadata exceeds its admitted bound");
  const unavailable = resultText !== undefined && bytes(resultText) > limit;
  if (unavailable) {
    delete receipt.result;
    receipt.error = "result body exceeds the admitted durable receipt limit";
  } else if (resultText !== undefined) receipt.result = raw(resultText);
  return { receipt, unavailable, body: wire("Receipt", receipt), usage: receipt.usage ? wire("Usage", receipt.usage) : "" };
}
