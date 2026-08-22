// Runs before the workload bundle's top-level code (ESM evaluation order).
if (typeof globalThis.TextEncoder === "undefined") {
  globalThis.TextEncoder = class TextEncoder {
    encode(s) {
      const bytes = unescape(encodeURIComponent(String(s)));
      const out = new Uint8Array(bytes.length);
      for (let i = 0; i < bytes.length; i++) out[i] = bytes.charCodeAt(i);
      return out;
    }
  };
}
if (typeof globalThis.structuredClone === "undefined") {
  globalThis.structuredClone = (v) => (v === undefined ? v : JSON.parse(JSON.stringify(v)));
}
