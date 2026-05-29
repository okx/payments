import type { ChallengeEcho } from './types.js'

/**
 * Build a `ChallengeEcho` for SA API requests.
 *
 * - Preserves `id` / `realm` / `method` / `intent` / `expires` from
 *   `credential.challenge` (already HMAC-verified by mppx).
 * - `request` is `base64url(JSON.stringify(request))` — SA API requires the
 *   request to be base64url-encoded JSON.
 * - The server-authoritative `request` replaces the client-echoed one to
 *   prevent client tampering of `methodDetails` etc.
 */
export function toChallengeEcho(
  credentialChallenge: Record<string, unknown>,
  authoritativeRequest: Record<string, unknown>,
): ChallengeEcho {
  return {
    id: credentialChallenge.id as string,
    realm: credentialChallenge.realm as string,
    method: credentialChallenge.method as string,
    intent: credentialChallenge.intent as string,
    request: base64urlEncode(JSON.stringify(authoritativeRequest)),
    expires: credentialChallenge.expires as string,
  }
}

/** Encode a UTF-8 string as base64url (no padding). */
function base64urlEncode(s: string): string {
  const bytes = new TextEncoder().encode(s)
  let binary = ''
  for (const b of bytes) binary += String.fromCodePoint(b)
  return globalThis.btoa(binary)
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '')
}
