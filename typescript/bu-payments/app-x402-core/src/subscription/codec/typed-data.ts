import { encodeAbiParameters, keccak256, type Hex, type TypedDataDomain } from "viem";

import type { PaymentRequirements } from "../../types/payments";
import type { CancelInitiator, PlanInitialCharge } from "../types";
import type { PermitSingleData, SubscriptionTerms } from "./payload";

/** 32-byte zero hash, used as the sentinel for empty bytes32 fields. */
export const ZERO_BYTES32: Hex = `0x${"0".repeat(64)}`;

/**
 * Permit2 PermitSingle (AllowanceTransfer) EIP-712 schema. Used for the
 * continuous-pull authorization signed by the buyer. The Permit2 domain has
 * NO `version` field — only `name="Permit2"`, `chainId`, `verifyingContract`.
 */
export const PERMIT2_TYPES = {
  PermitSingle: [
    { name: "details", type: "PermitDetails" },
    { name: "spender", type: "address" },
    { name: "sigDeadline", type: "uint256" },
  ],
  PermitDetails: [
    { name: "token", type: "address" },
    { name: "amount", type: "uint160" },
    { name: "expiration", type: "uint48" },
    { name: "nonce", type: "uint48" },
  ],
} as const;

/**
 * SubscriptionTerms EIP-712 schema. 17 fields; field ORDER + integer widths
 * must mirror the contract. TypeHash:
 *   0xa5223de56e7694cf776c7d4f74c0323f42bf9e65655fe49affefbdfd40ec97ae
 * (verify against contract `SUBSCRIPTION_TERMS_TYPEHASH()`)
 */
export const SUBSCRIPTION_TERMS_TYPES = {
  SubscriptionTerms: [
    { name: "payer", type: "address" },
    { name: "merchant", type: "address" },
    { name: "facilitator", type: "address" },
    { name: "token", type: "address" },
    { name: "amountPerPeriod", type: "uint160" },
    { name: "periodSec", type: "uint64" },
    { name: "maxPeriods", type: "uint32" },
    { name: "startAt", type: "uint64" },
    { name: "initialChargePeriods", type: "uint32" },
    { name: "initialChargeAmount", type: "uint160" },
    { name: "termsDeadline", type: "uint64" },
    { name: "permitHash", type: "bytes32" },
    { name: "salt", type: "bytes32" },
    { name: "planTier", type: "uint8" },
    { name: "changeFromSubId", type: "bytes32" },
    { name: "changeEffectiveAt", type: "uint8" },
    { name: "periodMode", type: "uint8" },
  ],
} as const;

/**
 * CancelAuth EIP-712 schema (subscription contract domain):
 *   `CancelAuth(uint8 action, bytes32 subId, uint8 initiator, bytes32 nonce, uint64 deadline)`
 */
export const CANCEL_AUTH_TYPES = {
  CancelAuth: [
    { name: "action", type: "uint8" },
    { name: "subId", type: "bytes32" },
    { name: "initiator", type: "uint8" },
    { name: "nonce", type: "bytes32" },
    { name: "deadline", type: "uint64" },
  ],
} as const;

/**
 * PendingChangeCancelAuth EIP-712 schema (subscription contract domain):
 *   `PendingChangeCancelAuth(bytes32 subId, bytes32 newSubId, bytes32 nonce, uint64 deadline)`
 */
export const PENDING_CHANGE_CANCEL_AUTH_TYPES = {
  PendingChangeCancelAuth: [
    { name: "subId", type: "bytes32" },
    { name: "newSubId", type: "bytes32" },
    { name: "nonce", type: "bytes32" },
    { name: "deadline", type: "uint64" },
  ],
} as const;

export interface SubscriptionPlanExtra {
  id: string;
  tier: number;
  name?: string;
}

/**
 * Buyer's intent to switch from `fromSubId` to a new plan. Present on
 * PaymentRequirements.extra ONLY when the requirements are a change offer
 * for an upgrade/downgrade.
 */
export interface ChangeFromExtra {
  fromSubId: Hex;
  fromPlanId: Hex;
  fromPlanTier: number;
  direction: "upgrade" | "downgrade";
  effectiveAt: "immediate" | "period_end";
}

export interface SubscriptionContractAddresses {
  subscription: string;
  permit2: string;
}

/**
 * Typed view of the `extra` field on subscription PaymentRequirements. The
 * evm scheme is responsible for producing PaymentRequirements whose `extra`
 * matches this shape; codec helpers consume it via this type rather than
 * Record<string, unknown> to keep field access type-safe.
 *
 * Index signature lets this be assigned directly to
 * `PaymentRequirements.extra: Record<string, unknown>` without a cast.
 */
export interface SubscriptionRequirementsExtra {
  contracts: SubscriptionContractAddresses;
  facilitator: string;
  amountPerPeriod: string;
  periodSec: number;
  /** uint8 — 0 fixed_seconds (default) / 1 calendar_month. */
  periodMode?: number;
  maxPeriods: number;
  startAt?: number;
  initialCharge?: PlanInitialCharge | null;
  plan: SubscriptionPlanExtra;
  changeFrom?: ChangeFromExtra;
  domain: TypedDataDomain;
  [key: string]: unknown;
}

export interface TypedDataEnvelope<TPrimary extends string, TMessage> {
  domain: TypedDataDomain;
  types: Record<string, ReadonlyArray<{ name: string; type: string }>>;
  primaryType: TPrimary;
  message: TMessage;
}

export interface BuildPermit2TypedDataInput {
  /** PaymentRequirements with subscription `extra`. */
  selected: PaymentRequirements;
  /** uint48 — current nonce from allowance-status (NOT +1). */
  nonce: number;
  /** uint48 unix seconds — Permit2 allowance expiration. */
  expiration: number;
  /** uint256 decimal — outer signature expiry. */
  sigDeadline: string;
  /** Override amount; default = initialChargeAmount + (maxPeriods - initialChargePeriods) * amountPerPeriod. */
  amount?: string;
}

export interface BuildSubscriptionTermsTypedDataInput {
  selected: PaymentRequirements;
  payer: string;
  startAt: number;
  /** uint64 unix seconds — terms signature validity. */
  termsDeadline: number;
  /** bytes32 — random anti-replay; reuse the same hex string ≤ once. */
  salt: Hex;
  /** bytes32 — keccak256 of the PermitSingle struct hash; bind permit. */
  permitHash: Hex;
  changeFrom?: ChangeFromExtra;
  /** Override domain for special cases; defaults to `selected.extra.domain`. */
  domain?: TypedDataDomain;
}

export interface BuildCancelAuthTypedDataInput {
  subId: Hex;
  initiator: CancelInitiator;
  /** uint64 unix seconds. */
  deadline: number;
  /** bytes32. */
  nonce: Hex;
  domain: TypedDataDomain;
}

export interface BuildPendingChangeCancelAuthTypedDataInput {
  subId: Hex;
  /** The pending change's target newSubId — must match `pendingPlanChange.newSubId`. */
  newSubId: Hex;
  deadline: number;
  nonce: Hex;
  domain: TypedDataDomain;
}

const CANCEL_INITIATOR_TO_ENUM: Record<CancelInitiator, 0 | 1> = {
  payer: 0,
  merchant: 1,
};

function getSubscriptionExtra(req: PaymentRequirements): SubscriptionRequirementsExtra {
  const extra = req.extra as Partial<SubscriptionRequirementsExtra> | undefined;
  if (!extra || !extra.contracts || !extra.plan || !extra.domain) {
    throw new Error(
      "subscription codec: PaymentRequirements.extra is missing contracts/plan/domain",
    );
  }
  return extra as SubscriptionRequirementsExtra;
}

/**
 * Default permit.amount:
 *   amount ≥ reservedAmount + newCommit
 *   newCommit = initialChargeAmount + (maxPeriods - initialChargePeriods) * amountPerPeriod
 *
 * `reservedAmount` (existing on-chain commitments) is NOT included here —
 * Buyer SDK should add it on top from `allowance-status.reservedAmount` if a
 * concurrent sub already holds budget against the same (payer, token).
 */
function defaultPermitAmount(extra: SubscriptionRequirementsExtra): string {
  const initialChargePeriods = BigInt(extra.initialCharge?.periodCount ?? 0);
  const initialChargeAmount = BigInt(extra.initialCharge?.totalAmount ?? "0");
  const remainingPeriods = BigInt(extra.maxPeriods) - initialChargePeriods;
  const remainingAmount =
    remainingPeriods > 0n ? remainingPeriods * BigInt(extra.amountPerPeriod) : 0n;
  return (initialChargeAmount + remainingAmount).toString();
}

/**
 * Build the Permit2 PermitSingle (AllowanceTransfer) typed data the wallet
 * signs. `spender = subscription contract`; `amount` covers the entire
 * remaining commitment (default), so the subscription contract can pull from
 * the same Permit2 allowance on every charge without re-signing.
 *
 * Permit2's EIP-712 domain has NO `version` field.
 */
export function buildPermit2TypedData(
  input: BuildPermit2TypedDataInput,
): TypedDataEnvelope<"PermitSingle", Record<string, unknown>> {
  const extra = getSubscriptionExtra(input.selected);
  const amount = input.amount ?? defaultPermitAmount(extra);
  const chainId = parseChainIdFromNetwork(input.selected.network);

  return {
    domain: {
      name: "Permit2",
      chainId,
      verifyingContract: extra.contracts.permit2 as Hex,
    },
    types: PERMIT2_TYPES as unknown as TypedDataEnvelope<
      "PermitSingle",
      Record<string, unknown>
    >["types"],
    primaryType: "PermitSingle",
    message: {
      details: {
        token: input.selected.asset,
        amount,
        expiration: input.expiration,
        nonce: input.nonce,
      },
      spender: extra.contracts.subscription,
      sigDeadline: input.sigDeadline,
    },
  };
}

/**
 * Compute the EIP-712 struct hash of a PermitSingle message — what
 * `terms.permitHash` must equal byte-for-byte. The facilitator verifies this
 * via `permit_hash_mismatch`.
 *
 *   permitDetailsHash = keccak256(
 *     PERMIT_DETAILS_TYPEHASH ‖ token ‖ amount ‖ expiration ‖ nonce
 *   )
 *   permitSingleHash  = keccak256(
 *     PERMIT_SINGLE_TYPEHASH ‖ permitDetailsHash ‖ spender ‖ sigDeadline
 *   )
 */
export function computePermitSingleStructHash(permit: PermitSingleData): Hex {
  // PermitDetails(address token,uint160 amount,uint48 expiration,uint48 nonce)
  const PERMIT_DETAILS_TYPEHASH = keccak256(
    new TextEncoder().encode(
      "PermitDetails(address token,uint160 amount,uint48 expiration,uint48 nonce)",
    ),
  );
  const detailsHash = keccak256(
    encodeAbiParameters(
      [
        { type: "bytes32" },
        { type: "address" },
        { type: "uint160" },
        { type: "uint48" },
        { type: "uint48" },
      ],
      [
        PERMIT_DETAILS_TYPEHASH,
        permit.details.token as Hex,
        BigInt(permit.details.amount),
        Number(permit.details.expiration),
        Number(permit.details.nonce),
      ],
    ),
  );

  // PermitSingle(PermitDetails details,address spender,uint256 sigDeadline)PermitDetails(...)
  const PERMIT_SINGLE_TYPEHASH = keccak256(
    new TextEncoder().encode(
      "PermitSingle(PermitDetails details,address spender,uint256 sigDeadline)" +
        "PermitDetails(address token,uint160 amount,uint48 expiration,uint48 nonce)",
    ),
  );
  return keccak256(
    encodeAbiParameters(
      [{ type: "bytes32" }, { type: "bytes32" }, { type: "address" }, { type: "uint256" }],
      [PERMIT_SINGLE_TYPEHASH, detailsHash, permit.spender as Hex, BigInt(permit.sigDeadline)],
    ),
  );
}

/**
 * Build the EIP-712 typed-data object the wallet signs to commit to the
 * subscription terms (17 fields). `permitHash` MUST be the struct hash of
 * the PermitSingle the buyer signed for permit2 (compute it via
 * `computePermitSingleStructHash`) — the contract verifies this binding to
 * defeat detached-signature swap attacks.
 */
export function buildSubscriptionTermsTypedData(
  input: BuildSubscriptionTermsTypedDataInput,
): TypedDataEnvelope<"SubscriptionTerms", SubscriptionTerms> {
  const extra = getSubscriptionExtra(input.selected);
  const domain = input.domain ?? extra.domain;

  // changeEffectiveAt: 0=none(create), 1=immediate(upgrade), 2=period_end(downgrade)
  let changeEffectiveAt = 0;
  if (input.changeFrom?.effectiveAt === "immediate") changeEffectiveAt = 1;
  else if (input.changeFrom?.effectiveAt === "period_end") changeEffectiveAt = 2;

  const message: SubscriptionTerms = {
    payer: input.payer,
    merchant: input.selected.payTo,
    facilitator: extra.facilitator,
    token: input.selected.asset,
    amountPerPeriod: extra.amountPerPeriod,
    periodSec: extra.periodSec,
    maxPeriods: extra.maxPeriods,
    startAt: input.startAt,
    initialChargePeriods: extra.initialCharge?.periodCount ?? 0,
    initialChargeAmount: extra.initialCharge?.totalAmount ?? "0",
    termsDeadline: input.termsDeadline,
    permitHash: input.permitHash,
    salt: input.salt,
    planTier: extra.plan.tier,
    changeFromSubId: input.changeFrom?.fromSubId ?? ZERO_BYTES32,
    changeEffectiveAt,
    periodMode: extra.periodMode ?? 0,
  };

  return {
    domain,
    types: SUBSCRIPTION_TERMS_TYPES as unknown as TypedDataEnvelope<
      "SubscriptionTerms",
      SubscriptionTerms
    >["types"],
    primaryType: "SubscriptionTerms",
    message,
  };
}

export function buildCancelAuthTypedData(
  input: BuildCancelAuthTypedDataInput,
): TypedDataEnvelope<"CancelAuth", Record<string, unknown>> {
  return {
    domain: input.domain,
    types: CANCEL_AUTH_TYPES as unknown as TypedDataEnvelope<
      "CancelAuth",
      Record<string, unknown>
    >["types"],
    primaryType: "CancelAuth",
    message: {
      action: 0,
      subId: input.subId,
      initiator: CANCEL_INITIATOR_TO_ENUM[input.initiator],
      nonce: input.nonce,
      deadline: input.deadline,
    },
  };
}

export function buildPendingChangeCancelAuthTypedData(
  input: BuildPendingChangeCancelAuthTypedDataInput,
): TypedDataEnvelope<"PendingChangeCancelAuth", Record<string, unknown>> {
  return {
    domain: input.domain,
    types: PENDING_CHANGE_CANCEL_AUTH_TYPES as unknown as TypedDataEnvelope<
      "PendingChangeCancelAuth",
      Record<string, unknown>
    >["types"],
    primaryType: "PendingChangeCancelAuth",
    message: {
      subId: input.subId,
      newSubId: input.newSubId,
      nonce: input.nonce,
      deadline: input.deadline,
    },
  };
}

/**
 * Parse the chainId from a CAIP-2 network identifier (`eip155:<chainId>`).
 * Throws if not an EVM CAIP — the subscription scheme is EVM-only.
 */
export function parseChainIdFromNetwork(network: string): number {
  const parts = network.split(":");
  if (parts.length !== 2 || parts[0] !== "eip155") {
    throw new Error(`parseChainIdFromNetwork: expected "eip155:<chainId>", got "${network}"`);
  }
  const id = Number(parts[1]);
  if (!Number.isInteger(id) || id <= 0) {
    throw new Error(`parseChainIdFromNetwork: invalid chainId "${parts[1]}"`);
  }
  return id;
}
