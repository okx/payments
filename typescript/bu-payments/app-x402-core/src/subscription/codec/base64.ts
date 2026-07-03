/**
 * Isomorphic UTF-8 ↔ base64 helpers.
 *
 * Buyers may run this code in a browser; Buffer is preferred on Node so the
 * built CJS bundle stays free of `globalThis.atob` polyfills, but we fall back
 * to atob/btoa when Buffer is unavailable.
 */

const hasBuffer = typeof Buffer !== "undefined";

export function base64EncodeUtf8(value: string): string {
  if (hasBuffer) return Buffer.from(value, "utf8").toString("base64");
  const binary = unescape(encodeURIComponent(value));
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return (globalThis as any).btoa(binary);
}

export function base64DecodeUtf8(value: string): string {
  if (hasBuffer) return Buffer.from(value, "base64").toString("utf8");
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const binary: string = (globalThis as any).atob(value);
  return decodeURIComponent(escape(binary));
}
