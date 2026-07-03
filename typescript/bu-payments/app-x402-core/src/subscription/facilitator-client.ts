import type { FacilitatorClient } from "../http/httpFacilitatorClient";
import type { PaymentPayload, PaymentRequirements } from "../types/payments";
import type { CancelAuth, PendingChangeCancelAuth, Subscription } from "./types";

/**
 * Standard OKX facilitator response envelope. `code === 0` means success
 * (NUMBER, not string).
 */
export interface FacilitatorEnvelope<T> {
  code: number;
  msg?: string | null;
  data?: T;
}

/** POST /api/v6/pay/x402/subscriptions response data. */
export interface FacilitatorSubscribeData {
  subId: string;
  txHash?: string;
  state: number;
}

/** POST /api/v6/pay/x402/subscriptions/change response data. */
export interface FacilitatorChangeData {
  newSubId: string;
  txHash?: string;
  state: number;
}

/** POST /api/v6/pay/x402/subscriptions/{id}/cancel response data. */
export interface FacilitatorCancelData {
  subId: string;
  txHash?: string;
  state: number;
}

/** POST /api/v6/pay/x402/subscriptions/{id}/cancel-pending-change response data. */
export interface FacilitatorCancelPendingData {
  subId: string;
  txHash?: string;
  state: number;
}

/** POST /api/v6/pay/x402/subscriptions/{id}/charge response data. */
export interface FacilitatorChargeData {
  subId: string;
  period: number;
  txHash?: string;
  /** SubscriptionChargeState — 0 pending / 1 success / 2 failed. */
  state: number;
  planChangeTriggered?: boolean;
  newSubId?: string | null;
}

/** POST /api/v6/pay/x402/subscriptions/{id}/finalize-expired response data. */
export interface FacilitatorFinalizeExpiredData {
  subId: string;
  txHash?: string;
  state: number;
}

/** One row of the charges feed (GET /api/v6/pay/x402/subscriptions/charges). */
export interface FacilitatorChargeRow {
  subId: string;
  period: number;
  /** 1 initial / 2 periodic / 3 downgrade_first_period / 4 finalize_expired_marker. */
  chargeType: number;
  amount: string;
  /** 0 pending / 1 success / 2 failed. */
  state: number;
  txHash?: string;
  planChangeTriggered?: boolean;
  newSubId?: string | null;
}

/** GET /api/v6/pay/x402/subscriptions/charges response data. */
export interface FacilitatorGetChargesData {
  charges: FacilitatorChargeRow[];
}

/** GET /api/v6/pay/x402/subscriptions/pending response data (most recent row). */
export interface FacilitatorPendingChangeRow {
  subId: string;
  newSubId: string;
  effectiveFromPeriod: number;
  /** 0 pending / 1 activated / 2 canceled / 3 expired. */
  state: number;
}

/** GET /api/v6/pay/x402/subscriptions/{id} response data. */
export interface FacilitatorGetSubscriptionData {
  subId: string;
  state: number;
  payer: string;
  merchant: string;
  token: string;
  amountPerPeriod: string;
  periodSec: number;
  /** 0 fixed_seconds / 1 calendar_month. */
  periodMode: number;
  maxPeriods: number;
  startAt: number;
  /** Calendar-month billing anchor (Unix s); 0/undefined in fixed_seconds mode. */
  billingAnchorAt?: number;
  /** Seller-side business identifier (NOT on-chain); facilitator echoes from its DB. */
  planId?: string;
  /** Plan tier from on-chain terms.planTier. */
  planTier?: number;
  lastChargedPeriod: number;
  totalPulled: string;
  changedToSubId?: string | null;
  isActive?: boolean;
  serviceEnded?: boolean;
  /** Mode-aware current period, clamped to maxPeriods (boundary = next period). */
  currentPeriod?: number;
  /**
   * Real elapsed period number, NOT clamped — `elapsedPeriods > maxPeriods`
   * means the service window already ended. SDK polls until
   * `lastChargedPeriod >= elapsedPeriods` to confirm a write settled.
   */
  elapsedPeriods?: number;
  nextChargeableAt?: number;
  pendingPlanChange?: {
    subId: string;
    newSubId: string;
    effectiveFromPeriod: number;
    state: number;
  } | null;
}

/**
 * Subscription write-flow request body shared by subscribe / change. The
 * facilitator parses these field-by-field — DO NOT wrap in an x402 envelope
 * (no `paymentPayload` / `paymentRequirements` wrapping).
 */
export interface SubscriptionWriteRequest {
  chainIndex: number;
  terms: PaymentPayload["payload"] extends { terms: infer T } ? T : unknown;
  permit: PaymentPayload["payload"] extends { permit: infer P } ? P : unknown;
  termsSig: string;
  permitSig: string;
  syncSettle?: boolean;
}

/** Subscription-aware facilitator client. Extends the base FacilitatorClient. */
export interface SubscriptionFacilitatorClient extends FacilitatorClient {
  /** POST /api/v6/pay/x402/subscriptions */
  subscribe(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorSubscribeData>>;

  /** POST /api/v6/pay/x402/subscriptions/change */
  changeSubscription(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
    oldSubId: string,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorChangeData>>;

  /** POST /api/v6/pay/x402/subscriptions/{subId}/cancel */
  cancelSubscription(
    subId: string,
    cancelAuth: CancelAuth,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorCancelData>>;

  /** POST /api/v6/pay/x402/subscriptions/{subId}/cancel-pending-change */
  cancelPendingChange(
    subId: string,
    cancelAuth: PendingChangeCancelAuth,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorCancelPendingData>>;

  /** POST /api/v6/pay/x402/subscriptions/{subId}/charge */
  chargeSubscription(
    subId: string,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorChargeData>>;

  /** POST /api/v6/pay/x402/subscriptions/finalize-expired — cleans up an ACTIVE sub whose service window has ended. */
  finalizeExpired(
    subId: string,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorFinalizeExpiredData>>;

  /** GET /api/v6/pay/x402/subscriptions/charges — paginated charge feed. */
  getCharges(
    subId: string,
    limit?: number,
    offset?: number,
  ): Promise<FacilitatorEnvelope<FacilitatorGetChargesData>>;

  /** GET /api/v6/pay/x402/subscriptions/pending — most recent pendingPlanChange row (any state). */
  getPendingChange(subId: string): Promise<FacilitatorEnvelope<FacilitatorPendingChangeRow | null>>;

  /** GET /api/v6/pay/x402/subscriptions/{subId} */
  getSubscription(subId: string): Promise<FacilitatorEnvelope<FacilitatorGetSubscriptionData>>;
}

/**
 * Type guard: does this FacilitatorClient implement the subscription
 * endpoints?
 */
export function supportsSubscription(
  client: FacilitatorClient,
): client is SubscriptionFacilitatorClient {
  const c = client as Partial<SubscriptionFacilitatorClient>;
  return (
    typeof c.subscribe === "function" &&
    typeof c.changeSubscription === "function" &&
    typeof c.cancelSubscription === "function" &&
    typeof c.cancelPendingChange === "function" &&
    typeof c.chargeSubscription === "function" &&
    typeof c.finalizeExpired === "function" &&
    typeof c.getCharges === "function" &&
    typeof c.getPendingChange === "function" &&
    typeof c.getSubscription === "function"
  );
}

/** Re-export `Subscription` so consumers don't have to import from `./types`. */
export type { Subscription };
