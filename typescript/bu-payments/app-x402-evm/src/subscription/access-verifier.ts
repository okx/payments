import { type Hex, recoverMessageAddress } from "viem";

import type { AccessProof } from "@okxweb3/app-x402-core/subscription";
import { buildAccessProofMessage } from "@okxweb3/app-x402-core/subscription";

export type AccessVerifyResult =
  | { ok: true; subId: string; payer: string }
  | { ok: false; error: string };

/**
 * Pure-cryptography verifier for the EIP-191 AccessProof. Does NOT touch the
 * SubscriptionStore or facilitator — the scheme is responsible for chaining
 * `verifier.verify(proof)` with `store.get(subId)` and tier-gate logic.
 *
 * Reasons we keep this as a separate helper class (design doc D7):
 *   - lets the scheme swap in a different proof scheme (SIWE / JWT) later
 *     without touching subscription orchestration
 *   - makes ecrecover unit-testable without store / facilitator mocks
 */
export class AccessProofVerifier {
  private readonly windowSec: number;

  constructor(opts: { windowSec?: number } = {}) {
    this.windowSec = opts.windowSec ?? 300;
  }

  async verify(proof: AccessProof): Promise<AccessVerifyResult> {
    if (!proof || proof.kind !== "subscription-id") {
      return { ok: false, error: "invalid_proof_kind" };
    }

    const now = Math.floor(Date.now() / 1000);
    if (Math.abs(proof.timestamp - now) > this.windowSec) {
      return { ok: false, error: "timestamp_out_of_window" };
    }

    let recovered: string;
    try {
      const message = buildAccessProofMessage({
        subId: proof.subId as Hex,
        payer: proof.payer as Hex,
        timestamp: proof.timestamp,
      });
      recovered = await recoverMessageAddress({
        message: { raw: message },
        signature: proof.signature as Hex,
      });
    } catch (err) {
      return {
        ok: false,
        error: `signature_recover_failed: ${(err as Error).message}`,
      };
    }

    if (recovered.toLowerCase() !== proof.payer.toLowerCase()) {
      return { ok: false, error: "signature_invalid" };
    }

    return { ok: true, subId: proof.subId, payer: proof.payer };
  }

  /** Window in seconds for replay protection. Exposed for tests / scheme. */
  getWindowSec(): number {
    return this.windowSec;
  }
}
