import { encodePacked, keccak256, type Hex } from "viem";

import type { AccessProof } from "../types";
import { base64DecodeUtf8, base64EncodeUtf8 } from "./base64";

export interface AccessProofMessageInput {
  subId: Hex; // bytes32
  payer: Hex; // address
  timestamp: number; // uint64, unix seconds
}

/**
 * Returns the inner 32-byte hash that wallets sign via EIP-191 personal_sign.
 *
 * Layout:
 *   inner = keccak256(abi.encodePacked(bytes32 subId, address payer, uint256 timestamp))
 *
 * `timestamp` is encoded as `uint256` (32 bytes). Switching to anything
 * narrower yields a different digest and silently produces
 * `signature_invalid` on every access call.
 *
 * IMPORTANT: callers must NOT prepend the EIP-191 envelope themselves —
 * `wallet.signMessage(buildAccessProofMessage(...))` will add the
 * `\x19Ethereum Signed Message:\n32` prefix automatically. The recover step on
 * the server must mirror this (verify EIP-191 signature, not raw keccak).
 */
export function buildAccessProofMessage(input: AccessProofMessageInput): Hex {
  return keccak256(
    encodePacked(
      ["bytes32", "address", "uint256"],
      [input.subId, input.payer, BigInt(input.timestamp)],
    ),
  );
}

export function encodeAccessProof(proof: AccessProof): string {
  return base64EncodeUtf8(JSON.stringify(proof));
}

export function decodeAccessProof(headerValue: string): AccessProof {
  const json = base64DecodeUtf8(headerValue);
  const parsed = JSON.parse(json) as AccessProof;
  if (!parsed || parsed.kind !== "subscription-id") {
    throw new Error(`decodeAccessProof: expected kind="subscription-id", got "${parsed?.kind}"`);
  }
  return parsed;
}
