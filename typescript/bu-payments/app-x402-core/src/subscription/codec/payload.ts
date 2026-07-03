import type { Hex } from "viem";

import type { PaymentPayload, PaymentRequired, PaymentRequirements } from "../../types/payments";
import { base64DecodeUtf8, base64EncodeUtf8 } from "./base64";

/**
 * Permit2 PermitSingle (AllowanceTransfer) — what the buyer signs to authorize
 * the subscription contract to pull tokens on every charge.
 *
 *   PermitSingle(PermitDetails details, address spender, uint256 sigDeadline)
 *   PermitDetails(address token, uint160 amount, uint48 expiration, uint48 nonce)
 */
export interface PermitSingleData {
  details: {
    token: string;
    /** uint160 as decimal string — covers entire remaining commitment. */
    amount: string;
    /** uint48 unix seconds — Permit2 allowance expiration. */
    expiration: number;
    /** uint48 — Permit2 nonce (current value from allowance-status, NOT +1). */
    nonce: number;
  };
  /** Subscription contract address. */
  spender: string;
  /** uint256 as decimal string — outer signature expiry. */
  sigDeadline: string;
}

/**
 * SubscriptionTerms — what the buyer's EIP-712 signature commits to. 17
 * fields. Field ORDER MATTERS because it determines the EIP-712 TypeHash:
 *   0xa5223de56e7694cf776c7d4f74c0323f42bf9e65655fe49affefbdfd40ec97ae
 */
export interface SubscriptionTerms {
  payer: string;
  merchant: string;
  facilitator: string;
  token: string;
  /** uint160 (atomic units). */
  amountPerPeriod: string;
  /** uint64 seconds; MUST be 0 when periodMode=1 (calendar_month). */
  periodSec: number;
  /** uint32. */
  maxPeriods: number;
  /** uint64 unix seconds; 0 means "use block.timestamp on-chain". */
  startAt: number;
  /** uint32; 0 = no separate initial charge. */
  initialChargePeriods: number;
  /** uint160; atomic units. */
  initialChargeAmount: string;
  /** uint64 unix seconds — terms signature validity window. */
  termsDeadline: number;
  /** bytes32 — keccak256 of the PermitSingle EIP-712 struct hash, binds terms to permit. */
  permitHash: Hex;
  /** bytes32 — buyer-generated random anti-replay value. */
  salt: Hex;
  /** uint8 (>0) — plan tier for upgrade/downgrade comparison. */
  planTier: number;
  /** bytes32 — old subId on change, zero on create. */
  changeFromSubId: Hex;
  /** uint8 — 0 none(create) / 1 immediate(upgrade) / 2 period_end(downgrade). */
  changeEffectiveAt: number;
  /** uint8 — 0 fixed_seconds / 1 calendar_month. */
  periodMode: number;
  /**
   * bytes32 — keccak256(utf8(plan.id)). Buyer SDKs sign it as a terms field
   * and the facilitator persists it as the on-chain `planId`. Seller verify
   * MUST cross-check this hash equals `keccak256(utf8(accepted.extra.plan.id))`
   * to defeat plan-spoof attacks.
   */
  planId?: Hex;
}

/**
 * Concrete payload nested inside `PaymentPayload.payload` for the `period`
 * scheme.
 */
export interface SubscriptionPaymentInner {
  permitSingle: PermitSingleData;
  permitSingleSignature: Hex;
  terms: SubscriptionTerms;
  termsSignature: Hex;
}

export interface EncodePaymentPayloadInput {
  selected: PaymentRequirements;
  permitSingle: PermitSingleData;
  permitSingleSignature: Hex;
  terms: SubscriptionTerms;
  termsSignature: Hex;
}

/**
 * Decode the `APP-PAYMENT-REQUIRED` header into the underlying
 * `PaymentRequired.accepts` array. Throws on invalid base64 / JSON.
 */
export function parsePaymentRequired(headerValue: string): PaymentRequirements[] {
  const json = base64DecodeUtf8(headerValue);
  const parsed = JSON.parse(json) as PaymentRequired;
  if (!parsed || !Array.isArray(parsed.accepts)) {
    throw new Error("parsePaymentRequired: missing or invalid `accepts` array");
  }
  return parsed.accepts;
}

/**
 * Build the `PAYMENT-SIGNATURE` header value (base64-encoded JSON). Buyer side.
 */
export function encodePaymentPayload(input: EncodePaymentPayloadInput): string {
  const payload: PaymentPayload = {
    x402Version: 2,
    accepted: input.selected,
    payload: {
      permitSingle: input.permitSingle,
      permitSingleSignature: input.permitSingleSignature,
      terms: input.terms,
      termsSignature: input.termsSignature,
    },
  };
  return base64EncodeUtf8(JSON.stringify(payload));
}

/**
 * Decode the `PAYMENT-SIGNATURE` header value back into a typed PaymentPayload.
 * Throws on invalid base64 / JSON; does not validate field shapes (that is the
 * responsibility of the scheme verify step).
 */
export function decodePaymentPayload(headerValue: string): PaymentPayload {
  const json = base64DecodeUtf8(headerValue);
  return JSON.parse(json) as PaymentPayload;
}

/**
 * Narrow a generic PaymentPayload's `.payload` to the subscription inner.
 * Throws if required fields are missing — single chokepoint between base64
 * decode and verify.
 */
export function asSubscriptionPaymentInner(payload: PaymentPayload): SubscriptionPaymentInner {
  const inner = payload.payload as Partial<SubscriptionPaymentInner>;
  if (
    !inner ||
    !inner.permitSingle ||
    !inner.terms ||
    !inner.permitSingleSignature ||
    !inner.termsSignature
  ) {
    throw new Error(
      "asSubscriptionPaymentInner: payload.payload is missing required permitSingle/terms fields",
    );
  }
  return inner as SubscriptionPaymentInner;
}
