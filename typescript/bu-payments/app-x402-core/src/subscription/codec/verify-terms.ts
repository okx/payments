import type { Hex } from "viem";

import type { PaymentRequirements } from "../../types/payments";
import { ErrorCode } from "../errors";
import type { SubscriptionTerms } from "./payload";
import { ZERO_BYTES32, type SubscriptionRequirementsExtra } from "./typed-data";

function addrEq(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

function hexEq(a: Hex | undefined, b: Hex): boolean {
  return !!a && a.toLowerCase() === b.toLowerCase();
}

/**
 * Verify that buyer-signed `terms` bind to the server-advertised
 * PaymentRequirements. Without this field-by-field check a buyer could sign
 * a basic-tier accepted but submit terms claiming enterprise-tier access at
 * basic price. The contract verifies the signature but cannot see what the
 * seller offered in 402 — only the SDK can enforce this binding.
 *
 * All mismatches collapse to `terms_binding_invalid` so the caller's switch
 * statement stays compact.
 *
 * Returns `null` on match, an `ErrorCode` on first mismatch.
 */
export function verifyTermsBindRequirements(
  terms: SubscriptionTerms,
  requirements: PaymentRequirements,
): ErrorCode | null {
  const extra = (requirements.extra ?? {}) as Partial<SubscriptionRequirementsExtra>;
  if (!extra.plan || extra.amountPerPeriod === undefined || extra.facilitator === undefined) {
    return ErrorCode.TermsBindingInvalid;
  }

  if (!addrEq(terms.merchant, requirements.payTo)) return ErrorCode.MerchantMismatch;
  if (!addrEq(terms.token, requirements.asset)) return ErrorCode.TermsBindingInvalid;
  if (!addrEq(terms.facilitator, extra.facilitator)) return ErrorCode.TermsBindingInvalid;

  if (terms.amountPerPeriod !== extra.amountPerPeriod) return ErrorCode.TermsBindingInvalid;
  if (terms.periodSec !== extra.periodSec) return ErrorCode.TermsBindingInvalid;
  if (terms.maxPeriods !== extra.maxPeriods) return ErrorCode.TermsBindingInvalid;
  if (terms.periodMode !== (extra.periodMode ?? 0)) return ErrorCode.TermsBindingInvalid;
  if (terms.planTier !== extra.plan.tier) return ErrorCode.TermsBindingInvalid;

  // Server-pinned startAt must be honored; if server left it unset (==undefined),
  // accept whatever the buyer signed (contract resolves 0 → block.timestamp).
  if (extra.startAt !== undefined && terms.startAt !== extra.startAt) {
    return ErrorCode.TermsBindingInvalid;
  }

  const expectedInitPeriods = extra.initialCharge?.periodCount ?? 0;
  const expectedInitAmount = extra.initialCharge?.totalAmount ?? "0";
  if (terms.initialChargePeriods !== expectedInitPeriods) return ErrorCode.TermsBindingInvalid;
  if (terms.initialChargeAmount !== expectedInitAmount) return ErrorCode.TermsBindingInvalid;

  // planId itself isn't compared here — the other field-level checks
  // (planTier / amountPerPeriod / merchant / …) already pin down the
  // economic effect of the subscription.

  // changeFrom binding — direction encoded in changeEffectiveAt:
  //   undefined → create (changeFromSubId==0 / changeEffectiveAt==0)
  //   immediate → upgrade (changeEffectiveAt==1)
  //   period_end → downgrade (changeEffectiveAt==2)
  if (extra.changeFrom) {
    if (!hexEq(terms.changeFromSubId, extra.changeFrom.fromSubId)) {
      return ErrorCode.TermsBindingInvalid;
    }
    const expectedEff =
      extra.changeFrom.effectiveAt === "immediate"
        ? 1
        : extra.changeFrom.effectiveAt === "period_end"
          ? 2
          : 0;
    if (terms.changeEffectiveAt !== expectedEff) return ErrorCode.TermsBindingInvalid;
  } else {
    if (terms.changeFromSubId !== ZERO_BYTES32) return ErrorCode.TermsBindingInvalid;
    if (terms.changeEffectiveAt !== 0) return ErrorCode.TermsBindingInvalid;
  }

  return null;
}
