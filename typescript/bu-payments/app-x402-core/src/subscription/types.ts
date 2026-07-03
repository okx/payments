import type { PaymentPayload, PaymentRequirements } from "../types/payments";

/**
 * Subscription state: `active`, `canceled`, `completed`, `changed`.
 */
/**
 * Local mirror of facilitator's SubscriptionState enum. Numeric mapping:
 *   0 pending / 1 active / 2 completed / 3 canceled / 4 changed / 99 failed
 */
export type SubscriptionState =
  | "pending"
  | "active"
  | "completed"
  | "canceled"
  | "changed"
  | "failed";

/**
 * Set on an ACTIVE sub when a downgrade has been scheduled but not yet
 * activated. `state` lets the seller observe the terminal disposition
 * (PENDING / ACTIVATED / CANCELED / EXPIRED) via GET /pending.
 */
export interface PendingPlanChange {
  subId: string;
  newSubId: string;
  effectiveFromPeriod: number;
  /** 0 pending / 1 activated / 2 canceled / 3 expired. */
  state: number;
}

/**
 * Seller-side projection of a subscription. All fields come from the
 * facilitator GET /subscriptions/detail endpoint; the store never holds
 * data the facilitator can't refresh.
 *
 * Snapshot fields (`isActive` / `serviceEnded` / `currentPeriod` /
 * `elapsedPeriods` / `nextChargeableAt`) are valid as of the last sync
 * only; they drift with wall-clock time.
 */
export interface Subscription {
  subId: string;
  payer: string;
  merchant: string;
  token: string;
  amountPerPeriod: string;
  /** 0 fixed_seconds / 1 calendar_month. */
  periodMode: number;
  periodSec: number;
  /** Calendar-month billing anchor (Unix s). Undefined or 0 in fixed_seconds mode. */
  billingAnchorAt?: number;
  maxPeriods: number;
  startAt: number;
  state: SubscriptionState;
  lastChargedPeriod: number;
  totalPulled: string;
  planId: string;
  planTier: number;
  changedToSubId?: string;
  pendingPlanChange?: PendingPlanChange;
  /** Derived snapshot — true iff state==ACTIVE && now < endAt. */
  isActive?: boolean;
  /** Derived snapshot — true iff state==ACTIVE && !isActive (expired, not yet finalized). */
  serviceEnded?: boolean;
  /** Derived snapshot — current period number, clamped to maxPeriods. */
  currentPeriod?: number;
  /** Derived snapshot — real elapsed period count, NOT clamped (use this for expiry checks). */
  elapsedPeriods?: number;
  /** Derived snapshot — next chargeable boundary (Unix s); null when all periods are charged. */
  nextChargeableAt?: number;
}

export interface AccessProof {
  kind: "subscription-id";
  subId: string;
  payer: string;
  timestamp: number;
  signature: string;
}

/**
 * `CancelAuth.initiator` enum — only payer / merchant.
 */
export type CancelInitiator = "payer" | "merchant";

/**
 * EIP-712 `CancelAuth` payload (subscription contract domain):
 *   `CancelAuth(uint8 action, bytes32 subId, uint8 initiator, bytes32 nonce, uint64 deadline)`
 *
 * `action` is locked to `0 = cancel_subscription`. `cancel_pending_change`
 * uses the standalone `cancel-pending-change` endpoint with its own
 * `PendingChangeCancelAuth` signature.
 */
export interface CancelAuth {
  action: 0;
  subId: string;
  initiator: 0 | 1;
  nonce: string;
  deadline: number;
  signature: string;
}

/**
 * EIP-712 `PendingChangeCancelAuth` payload (subscription contract domain).
 * Payer-only.
 *
 * TypeHash:
 *   keccak256("PendingChangeCancelAuth(bytes32 subId,bytes32 newSubId,bytes32 nonce,uint64 deadline)")
 *
 * `newSubId` MUST equal the currently-PENDING `pendingPlanChange.newSubId`
 * (the to-be-cancelled downgrade target); facilitator rejects with
 * `pending_cancel_target_mismatch` otherwise.
 */
export interface PendingChangeCancelAuth {
  subId: string;
  newSubId: string;
  nonce: string;
  deadline: number;
  signature: string;
}

export interface PlanInitialCharge {
  periodCount: number;
  totalAmount: string;
}

export interface PlanCatalogEntry {
  id: string;
  tier: number;
  amountPerPeriod: string;
  /** 0 fixed_seconds (default) / 1 calendar_month. */
  periodMode?: 0 | 1;
  periodSec: number;
  maxPeriods: number;
  /**
   * ERC-20 token address. Optional — if omitted, the EVM scheme fills from
   * `getDefaultAsset(network)` (same per-network map exact / upto /
   * aggr_deferred consume).
   */
  asset?: string;
  payTo: string;
  initialCharge?: PlanInitialCharge;
  name?: string;
}

export type PlanCatalog = Record<string, PlanCatalogEntry>;

export interface AccessRouteRequirements {
  /**
   * PlanIds that satisfy this route. Derived from the route's `accepts`
   * payment options (`accepts[].extra.plan.id`). A subscription is allowed
   * access iff its `planId` appears in this list.
   *
   * `undefined` means "no plan restriction" — any active subscription on
   * the route passes (use sparingly).
   */
  acceptedPlanIds?: string[];

  /**
   * Full `PaymentRequirements` for every plan the route accepts — the same
   * list the seller declared as `RouteConfig.accepts`, resolved to wire
   * format. Each entry carries the plan metadata in `extra.plan`
   * (`{ id, tier, name }`) plus `extra.amountPerPeriod`, `extra.periodSec`,
   * `extra.periodMode`, `extra.maxPeriods`, etc. — everything an
   * `onBeforeAccess` hook needs to decide policy against catalog details
   * (upgrade offers, tier ceilings, per-plan feature flags) without joining
   * an external catalog table.
   */
  accepts?: PaymentRequirements[];
}

/**
 * Context passed to `OnBeforeAccessHook`. Carries the full stored
 * `Subscription` (so the seller can inspect any field — payer, planId,
 * lastChargedPeriod, changedToSubId, etc. — for arbitrary policy) plus
 * the incoming HTTP request shape and route metadata.
 */
export interface OnBeforeAccessContext {
  subscription: Subscription;
  request: {
    path: string;
    method: string;
    headers: Record<string, string>;
  };
  route: AccessRouteRequirements;
}

/**
 * Result of an `OnBeforeAccessHook`:
 *   - `{ ok: true }`  → allow the request through
 *   - `{ ok: false }` → deny; `error` shows up in the 402 body, `retryAfter`
 *     (seconds) becomes a `Retry-After` header hint, `upgradeOffers` lets
 *     the seller point the buyer at an alternate plan
 *
 * Denial use cases: rate-limiting, quota exhaustion, bans / blacklists,
 * per-plan feature gating beyond the simple `acceptedPlanIds` allowlist.
 */
export type OnBeforeAccessResult =
  | { ok: true }
  | {
      ok: false;
      error?: string;
      retryAfter?: number;
      upgradeOffers?: PaymentRequirements[];
    };

/**
 * Route-level hook fired AFTER `verifyAccess` succeeded (signature +
 * payer + plan-allowlist + period math) but BEFORE the handler runs.
 * The seller uses it to implement custom access policy — e.g. a ban list
 * keyed by subId or payer, per-plan feature flags, or dynamic quota.
 */
export type OnBeforeAccessHook = (ctx: OnBeforeAccessContext) => Promise<OnBeforeAccessResult>;

export type VerifyResultOk = { ok: true };
export type VerifyResultFail = { ok: false; error: string };
export type VerifyResult = VerifyResultOk | VerifyResultFail;

export interface VerifyChangeOk {
  ok: true;
  oldSubId: string;
  direction: "upgrade" | "downgrade";
}
export type VerifyChangeResult = VerifyChangeOk | VerifyResultFail;

export interface VerifyAccessOk {
  ok: true;
  subscription: Subscription;
}
export type VerifyAccessResult = VerifyAccessOk | VerifyResultFail;

/**
 * Result of `verifyOwnership` — used by the change-route sniff path to
 * confirm the AccessProof signer owns the named subscription, without
 * imposing the plan-allowlist / period-math gating that `verifyAccess`
 * applies for resource consumption.
 */
export interface VerifyOwnershipOk {
  ok: true;
  subId: string;
  payer: string;
  subscription: Subscription;
}
export type VerifyOwnershipResult = VerifyOwnershipOk | VerifyResultFail;

export type SettleResultFail = {
  success: false;
  error: string;
  /**
   * Set when the chain operation may still complete asynchronously: the
   * facilitator accepted the write call but returned `state=pending`, and the
   * SDK's client-side polling (5×1s) timed out before settlement. Seller
   * should remember `subId` and call `syncFromChain(subId)` later.
   */
  subId?: string;
  pending?: boolean;
};

export interface SettleSubscribeOk {
  success: true;
  subId: string;
  subscription: Subscription;
  headers: Record<string, string>;
}
export type SettleSubscribeResult = SettleSubscribeOk | SettleResultFail;

export interface SettleChangeOk {
  success: true;
  oldSubId: string;
  newSubId: string;
  operationType: "upgrade" | "downgrade";
  scheduledFromPeriod?: number;
  headers: Record<string, string>;
}
export type SettleChangeResult = SettleChangeOk | SettleResultFail;

export interface SettleCancelOk {
  success: true;
  subId: string;
  headers: Record<string, string>;
}
export type SettleCancelResult = SettleCancelOk | SettleResultFail;

/**
 * Result of `settleCancelPendingChange` — cancels a scheduled downgrade
 * (removes `pendingPlanChange` on the old sub while the sub itself stays
 * ACTIVE). No refund, no state transition on the sub itself.
 */
export interface SettleCancelPendingChangeOk {
  success: true;
  subId: string;
  headers: Record<string, string>;
}
export type SettleCancelPendingChangeResult = SettleCancelPendingChangeOk | SettleResultFail;

export interface ChargeResult {
  success: true;
  period: number;
  amount: string;
  txHash?: string;
  planChangeTriggered?: boolean;
  newSubId?: string;
}

export interface SubscriptionCapability {
  readonly settlementMode: "pre";

  verifySubscribe(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<VerifyResult>;

  settleSubscribe(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<SettleSubscribeResult>;

  /**
   * Inject `extra.changeFrom = { fromSubId, fromPlanId, fromPlanTier,
   * direction, effectiveAt }` into each accept of a change route's 402
   * accepts. Direction / effectiveAt are derived per-accept by comparing
   * `accept.extra.plan.tier` against the stored `oldSub.planTier`. Same-tier
   * accepts are dropped (a change to the same tier is illegal —
   * `tier_same`).
   *
   * Returns `null` when the seller's local store has no record of
   * `currentSubId` — middleware then 404s the GET so buyers see the misuse.
   */
  enrichAcceptsForChange(
    accepts: PaymentRequirements[],
    currentSubId: string,
  ): Promise<PaymentRequirements[] | null>;

  verifyChange(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<VerifyChangeResult>;

  settleChange(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<SettleChangeResult>;

  verifyCancel(auth: CancelAuth, subId: string): Promise<VerifyResult>;

  settleCancel(auth: CancelAuth, subId: string): Promise<SettleCancelResult>;

  /**
   * Verify a `PendingChangeCancelAuth` before facilitator submission.
   * Requires `auth.subId == body.subId` and `auth.newSubId` equal to the
   * currently PENDING `pendingPlanChange.newSubId` — the SDK checks the
   * former; facilitator enforces the latter as
   * `pending_cancel_target_mismatch`.
   */
  verifyCancelPendingChange(auth: PendingChangeCancelAuth, subId: string): Promise<VerifyResult>;

  /**
   * Cancel a scheduled downgrade (`pendingPlanChange`) — the current sub
   * stays ACTIVE, only the pending row is retired. Facilitator returns the
   * new state; SDK re-pulls GET /detail to refresh the store entry.
   */
  settleCancelPendingChange(
    auth: PendingChangeCancelAuth,
    subId: string,
  ): Promise<SettleCancelPendingChangeResult>;

  verifyAccess(proof: AccessProof, route: AccessRouteRequirements): Promise<VerifyAccessResult>;

  /**
   * Lightweight ownership check for the change-route sniff path. Verifies
   * the AccessProof signature, looks up the sub in the store, and confirms
   * `sub.payer == proof.payer`. Deliberately does NOT enforce plan
   * allowlist or period math — the caller is identifying themselves to
   * receive change offers, not consuming a protected resource.
   */
  verifyOwnership(proof: AccessProof): Promise<VerifyOwnershipResult>;

  charge(subId: string): Promise<ChargeResult>;

  getSubscription(subId: string): Promise<Subscription | null>;
}

export function hasSubscriptionCapability(scheme: unknown): scheme is SubscriptionCapability {
  return (
    typeof scheme === "object" &&
    scheme !== null &&
    "verifyAccess" in scheme &&
    "settlementMode" in scheme &&
    (scheme as { settlementMode: unknown }).settlementMode === "pre"
  );
}
