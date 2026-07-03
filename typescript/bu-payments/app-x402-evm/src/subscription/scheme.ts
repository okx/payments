import type { TypedDataDomain } from "viem";

import {
  asSubscriptionPaymentInner,
  ChargeError,
  ChargeErrorCode,
  computeElapsedPeriods,
  ErrorCode,
  parseChainIdFromNetwork,
  supportsSubscription,
  verifyTermsBindRequirements,
  ZERO_BYTES32,
  type AccessProof,
  type AccessRouteRequirements,
  type CancelAuth,
  type ChargeResult,
  type PendingChangeCancelAuth,
  type SettleCancelPendingChangeResult,
  type SettleCancelResult,
  type SettleChangeResult,
  type SettleSubscribeResult,
  type Subscription,
  type SubscriptionCapability,
  type SubscriptionFacilitatorClient,
  type SubscriptionState,
  type SubscriptionStore,
  type VerifyAccessResult,
  type VerifyChangeResult,
  type VerifyOwnershipResult,
  type VerifyResult,
} from "@okxweb3/app-x402-core/subscription";
import type {
  AssetAmount,
  Network,
  PaymentPayload,
  PaymentRequirements,
  Price,
  SchemeNetworkServer,
} from "@okxweb3/app-x402-core/types";

import { SUBSCRIPTION_DOMAIN_NAME, SUBSCRIPTION_DOMAIN_VERSION } from "../constants";
import { getDefaultAsset } from "../shared/defaultAssets";
import { AccessProofVerifier } from "./access-verifier";

export interface PermitSubscriptionSchemeConfig {
  /**
   * Facilitator client implementing `SubscriptionFacilitatorClient`.
   * `HTTPFacilitatorClient` and `OKXFacilitatorClient` both qualify.
   * All facilitator HTTP / auth / retry concerns live inside this client —
   * the scheme is pure protocol logic on top of it.
   *
   * The facilitator's on-chain identity (its EOA address, used as the
   * `extra.facilitator` field in PaymentRequirements) is read from the
   * `signers` map returned by `getSupported()`, cached on first use.
   */
  facilitator: SubscriptionFacilitatorClient;
  network: Network;
  store: SubscriptionStore;
  /** Override AccessProof replay window. Default: 300s. */
  accessProofWindowSec?: number;
}

/**
 * Map a facilitator-returned `reason` string (or msg fallback) into one of the
 * 6 well-known `ChargeErrorCode` values. Unknown strings fall back to
 * `ConfirmationTimeout` — the most conservative "we don't know what happened,
 * call syncFromChain" code.
 */
function mapToChargeError(reason: string | undefined): ChargeErrorCode {
  switch (reason) {
    case ChargeErrorCode.PeriodNotDue:
    case ChargeErrorCode.SubscriptionNotActive:
    case ChargeErrorCode.InsufficientBalance:
    case ChargeErrorCode.AllowanceExpired:
    case ChargeErrorCode.UnauthorizedCaller:
    case ChargeErrorCode.ConfirmationTimeout:
      return reason;
    default:
      return ChargeErrorCode.ConfirmationTimeout;
  }
}

/**
 * period scheme — Seller-side implementation.
 *
 * One class implements:
 *   1. SchemeNetworkServer        — registered into x402ResourceServer for
 *                                   PaymentRequirements production
 *   2. SubscriptionCapability     — duck-typed by dispatch via
 *                                   `hasSubscriptionCapability(scheme)`
 *   3. Facilitator HTTP caller    — wraps `/api/v6/pay/x402/subscriptions*`
 */
export class PermitSubscriptionScheme implements SchemeNetworkServer, SubscriptionCapability {
  readonly scheme = "period";
  readonly settlementMode = "pre" as const;

  protected readonly facilitator: SubscriptionFacilitatorClient;
  protected readonly network: Network;
  protected readonly store: SubscriptionStore;
  protected readonly accessVerifier: AccessProofVerifier;

  constructor(config: PermitSubscriptionSchemeConfig) {
    if (!supportsSubscription(config.facilitator)) {
      throw new Error(
        "PermitSubscriptionScheme: injected facilitator does not implement " +
          "SubscriptionFacilitatorClient (subscribe / changeSubscription / " +
          "cancelSubscription / chargeSubscription / getSubscription). " +
          "Use HTTPFacilitatorClient or OKXFacilitatorClient.",
      );
    }
    this.facilitator = config.facilitator;
    this.network = config.network;
    this.store = config.store;
    this.accessVerifier = new AccessProofVerifier({
      windowSec: config.accessProofWindowSec,
    });
  }

  /**
   * Extract (facilitator, subscriptionContract, permit2Contract) from the
   * `supportedKind` that x402ResourceServer hands us — its `extra` already
   * holds the cached `/supported` response. NO network call.
   *
   * The result is also memoized so synchronous callers (CancelAuth signing
   * in demo, e.g. via `getDomain()`) can read after the first enhance call.
   */
  private resolveFromSupportedKind(supportedKind: { extra?: Record<string, unknown> }): {
    facilitator: string;
    contracts: { subscription: string; permit2: string };
    domain: TypedDataDomain;
  } {
    const extra = supportedKind.extra as
      | { facilitatorAddress?: string; subscriptionContract?: string; permit2Contract?: string }
      | undefined;
    if (!extra?.facilitatorAddress || !extra.subscriptionContract || !extra.permit2Contract) {
      throw new Error(
        `period supportedKind.extra is missing required fields ` +
          `(facilitatorAddress / subscriptionContract / permit2Contract) for ${this.network}`,
      );
    }
    const resolved = {
      facilitator: extra.facilitatorAddress,
      contracts: {
        subscription: extra.subscriptionContract,
        permit2: extra.permit2Contract,
      },
      domain: {
        name: SUBSCRIPTION_DOMAIN_NAME,
        version: SUBSCRIPTION_DOMAIN_VERSION,
        chainId: parseChainIdFromNetwork(this.network),
        verifyingContract: extra.subscriptionContract as `0x${string}`,
      },
    };
    this.lastResolved = resolved;
    return resolved;
  }

  /**
   * Memoized snapshot from the most recent `enhancePaymentRequirements`
   * call. Synchronous accessors (`getDomain`) read from here. Populated
   * implicitly by the first request; resourceServer.initialize() runs
   * before any request, so by the time you sign a CancelAuth (which happens
   * after a buyer flow has at least once succeeded) this is set.
   */
  protected lastResolved?: {
    facilitator: string;
    contracts: { subscription: string; permit2: string };
    domain: TypedDataDomain;
  };

  /** Read-only accessor for the EIP-712 domain (uses memoized snapshot). */
  getDomain(): TypedDataDomain {
    if (!this.lastResolved) {
      throw new Error(
        "PermitSubscriptionScheme.getDomain(): scheme hasn't seen a request yet — " +
          "domain comes from the cached /supported response, which the resource server " +
          "passes via enhancePaymentRequirements. Trigger one buyer request first, or " +
          "call buildPaymentRequirements eagerly on boot.",
      );
    }
    return this.lastResolved.domain;
  }

  // ── SchemeNetworkServer ────────────────────────────────────────────────
  async parsePrice(price: Price, _network: Network): Promise<AssetAmount> {
    if (typeof price === "object" && price !== null && "asset" in price && "amount" in price) {
      return { asset: price.asset, amount: price.amount, extra: price.extra };
    }
    throw new Error(
      "period.parsePrice: price must be a PlanCatalog-derived AssetAmount; " +
        "use plan.amountPerPeriod + plan.asset rather than Money strings",
    );
  }

  async enhancePaymentRequirements(
    paymentRequirements: PaymentRequirements,
    supportedKind: {
      x402Version: number;
      scheme: string;
      network: Network;
      extra?: Record<string, unknown>;
    },
    _facilitatorExtensions: string[],
  ): Promise<PaymentRequirements> {
    // Pull the protocol-fixed extras from the supportedKind that
    // x402ResourceServer cached at initialize() time. NO network call here.
    // Each enhance call sees the SAME data → no race, no flap.
    const { facilitator, contracts, domain } = this.resolveFromSupportedKind(supportedKind);
    return {
      ...paymentRequirements,
      extra: {
        ...(paymentRequirements.extra ?? {}),
        contracts,
        facilitator,
        domain,
      },
    };
  }

  // ── SubscriptionCapability: Subscribe ──────────────────────────────────
  async verifySubscribe(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<VerifyResult> {
    // The contract verifies signatures / deadlines / permitHash / salt, but
    // it can't see what the seller actually offered in 402. The SDK is the
    // only place that can bind buyer-signed terms back to the advertised
    // PaymentRequirements.
    let inner;
    try {
      inner = asSubscriptionPaymentInner(payload);
    } catch {
      return { ok: false, error: ErrorCode.TermsBindingInvalid };
    }
    const bindErr = verifyTermsBindRequirements(inner.terms, requirements);
    if (bindErr) return { ok: false, error: bindErr };
    return { ok: true };
  }

  async settleSubscribe(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<SettleSubscribeResult> {
    let envelope;
    try {
      envelope = await this.facilitator.subscribe(payload, requirements);
    } catch {
      return { success: false, error: ErrorCode.ConfirmationTimeout };
    }

    if (envelope.code !== 0 || !envelope.data) {
      return { success: false, error: envelope.msg ?? ErrorCode.ConfirmationTimeout };
    }

    // GET /detail is the sole source of truth for the store. Poll until the
    // facilitator reports the first period has been charged
    // (`lastChargedPeriod >= elapsedPeriods`); on timeout surface
    // pending+subId so the seller can retry / syncFromChain later.
    const subId = envelope.data.subId;
    const settled = await this.pollUntilSettled(subId);
    if (!settled) {
      return { success: false, error: "pending", subId, pending: true };
    }
    const subscription = this.subscriptionFromGetResp(settled);
    await this.store.put(subscription);

    return {
      success: true,
      subId: subscription.subId,
      subscription,
      headers: {
        "PAYMENT-RESPONSE": Buffer.from(
          JSON.stringify({ subId: subscription.subId, txHash: envelope.data.txHash }),
          "utf8",
        ).toString("base64"),
      },
    };
  }

  // ── SubscriptionCapability: Ownership (change-route sniff) ────────────
  /**
   * Confirms the AccessProof signer owns the named subscription. Used by
   * the change-route sniff path — the buyer is identifying themselves, not
   * consuming a protected resource, so the heavyweight period / plan
   * checks from `verifyAccess` are deliberately skipped.
   */
  async verifyOwnership(proof: AccessProof): Promise<VerifyOwnershipResult> {
    const v = await this.accessVerifier.verify(proof);
    if (!v.ok) return v;
    const sub = await this.store.get(proof.subId);
    if (!sub) return { ok: false, error: ErrorCode.SubscriptionNotActive };
    if (sub.payer.toLowerCase() !== proof.payer.toLowerCase()) {
      return { ok: false, error: ErrorCode.PayerMismatch };
    }
    return { ok: true, subId: proof.subId, payer: proof.payer, subscription: sub };
  }

  // ── SubscriptionCapability: Access ─────────────────────────────────────
  /**
   * AccessProof gate. State is decided by period math, NOT by
   * `sub.state` — a changed/canceled sub naturally fails the period check
   * because its `lastChargedPeriod` is frozen while `currentCalculatePeriod`
   * keeps advancing.
   *
   * Flow:
   *   1. ecrecover the AccessProof (time-window + signer == payer).
   *   2. Sub must exist in store; payer + plan allowlist match.
   *   3. Compute `currentCalculatePeriod` locally.
   *   4. `currentCalculatePeriod === 0` → pre-start, return
   *      `subscription_not_yet_active`.
   *   5. `lastChargedPeriod >= currentCalculatePeriod` → allow.
   *   6. Otherwise fall through to GET /detail (facilitator authoritative),
   *      refresh store, and re-check against `data.elapsedPeriods`.
   */
  async verifyAccess(
    proof: AccessProof,
    route: AccessRouteRequirements,
  ): Promise<VerifyAccessResult> {
    const v = await this.accessVerifier.verify(proof);
    if (!v.ok) return v;

    let sub = await this.store.get(proof.subId);
    if (!sub) return { ok: false, error: ErrorCode.SubscriptionNotActive };

    if (sub.payer.toLowerCase() !== proof.payer.toLowerCase()) {
      return { ok: false, error: ErrorCode.PayerMismatch };
    }
    if (route.acceptedPlanIds && !route.acceptedPlanIds.includes(sub.planId)) {
      return { ok: false, error: ErrorCode.SubscriptionNotActive };
    }

    const now = Math.floor(Date.now() / 1000);
    const currentCalculatePeriod = computeElapsedPeriods(
      sub.periodMode,
      sub.startAt,
      sub.billingAnchorAt ?? 0,
      sub.periodSec,
      now,
    );
    if (currentCalculatePeriod === 0) {
      return { ok: false, error: ErrorCode.SubscriptionNotYetActive };
    }
    if (sub.lastChargedPeriod >= currentCalculatePeriod) {
      return { ok: true, subscription: sub };
    }

    // Local view says we're behind — pull facilitator-authoritative values.
    const detail = await this.getDetailOnce(proof.subId);
    if (!detail) return { ok: false, error: ErrorCode.SubscriptionNotActive };
    sub = this.subscriptionFromGetResp(detail);
    await this.store.put(sub);

    const remoteElapsed = detail.elapsedPeriods ?? 0;
    if (remoteElapsed === 0) return { ok: false, error: ErrorCode.SubscriptionNotYetActive };
    if (sub.lastChargedPeriod >= remoteElapsed) return { ok: true, subscription: sub };
    return { ok: false, error: ErrorCode.SubscriptionNotActive };
  }

  // ── SubscriptionCapability: getSubscription (chain query) ──────────────
  async getSubscription(subId: string): Promise<Subscription | null> {
    let envelope;
    try {
      envelope = await this.facilitator.getSubscription(subId);
    } catch (err) {
      throw new Error(`getSubscription failed for ${subId}: ${(err as Error).message}`);
    }
    if (envelope.code !== 0 || !envelope.data) return null;
    return this.subscriptionFromGetResp(envelope.data);
  }

  // ── SubscriptionCapability: Change ─────────────────────────────────────
  // The change route's `accepts` already lists every plan the buyer can
  // switch to. Buyer fetches 402 from the change route, picks one, signs
  // newTerms with `changeFromSubId` = current subId.
  async enrichAcceptsForChange(
    accepts: PaymentRequirements[],
    currentSubId: string,
  ): Promise<PaymentRequirements[] | null> {
    const oldSub = await this.store.get(currentSubId);
    if (!oldSub) return null;
    const enriched: PaymentRequirements[] = [];
    for (const accept of accepts) {
      const extra = (accept.extra ?? {}) as { plan?: { id?: string; tier?: number } } & Record<
        string,
        unknown
      >;
      const newTier = extra.plan?.tier;
      if (typeof newTier !== "number") continue;
      if (newTier === oldSub.planTier) continue; // tier_same — drop this accept
      const direction: "upgrade" | "downgrade" =
        newTier > oldSub.planTier ? "upgrade" : "downgrade";
      const effectiveAt: "immediate" | "period_end" =
        direction === "upgrade" ? "immediate" : "period_end";
      enriched.push({
        ...accept,
        extra: {
          ...extra,
          changeFrom: {
            fromSubId: oldSub.subId,
            fromPlanId: oldSub.planId,
            fromPlanTier: oldSub.planTier,
            direction,
            effectiveAt,
          },
        },
      });
    }
    return enriched;
  }

  async verifyChange(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<VerifyChangeResult> {
    // Contract owns signature / deadline / permitHash / tier-direction /
    // credit checks. SDK enforces the terms↔requirements binding the
    // contract can't see (same rationale as verifySubscribe), plus the
    // change-specific store lookup so dispatch can route to settleChange
    // with a resolved old subId.
    let inner;
    try {
      inner = asSubscriptionPaymentInner(payload);
    } catch {
      return { ok: false, error: ErrorCode.TermsBindingInvalid };
    }

    const bindErr = verifyTermsBindRequirements(inner.terms, requirements);
    if (bindErr) return { ok: false, error: bindErr };

    const changeFromHash = inner.terms.changeFromSubId;
    if (!changeFromHash || changeFromHash === ZERO_BYTES32) {
      return { ok: false, error: ErrorCode.TermsBindingInvalid };
    }

    const oldSub = await this.store.get(changeFromHash);
    if (!oldSub) return { ok: false, error: ErrorCode.SubNotActiveForChange };

    const direction: "upgrade" | "downgrade" =
      inner.terms.planTier > oldSub.planTier ? "upgrade" : "downgrade";
    return { ok: true, oldSubId: changeFromHash, direction };
  }

  async settleChange(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<SettleChangeResult> {
    const inner = asSubscriptionPaymentInner(payload);
    const oldSubId = inner.terms.changeFromSubId;
    const oldSub = await this.store.get(oldSubId);
    const direction: "upgrade" | "downgrade" =
      oldSub && inner.terms.planTier > oldSub.planTier ? "upgrade" : "downgrade";

    let envelope;
    try {
      envelope = await this.facilitator.changeSubscription(payload, requirements, oldSubId);
    } catch {
      return { success: false, error: ErrorCode.ConfirmationTimeout };
    }
    if (envelope.code !== 0 || !envelope.data) {
      return { success: false, error: envelope.msg ?? ErrorCode.ConfirmationTimeout };
    }
    const data = envelope.data;

    if (direction === "upgrade") {
      // Upgrade: new sub's first period is charged on-chain atomically; poll
      // until lastChargedPeriod >= elapsedPeriods on GET /detail of the new
      // subId. Order: put new first, then mutate old — if we crash midway,
      // both surfaces stay observable instead of leaving `changedToSubId`
      // dangling at a missing entry.
      const settled = await this.pollUntilSettled(data.newSubId);
      if (!settled) {
        return { success: false, error: "pending", subId: data.newSubId, pending: true };
      }
      await this.store.put(this.subscriptionFromGetResp(settled));
      if (oldSub) {
        await this.store.put({
          ...oldSub,
          state: "changed",
          changedToSubId: data.newSubId,
        });
      }
    } else {
      // Downgrade: only schedules a pendingPlanChange — no first-period
      // charge to wait for. Pull the old sub's authoritative state once
      // (pendingPlanChange.effectiveFromPeriod is set by the facilitator,
      // not derivable locally).
      const detail = await this.getDetailOnce(oldSubId);
      if (detail) {
        await this.store.put(this.subscriptionFromGetResp(detail));
      }
    }

    return {
      success: true,
      oldSubId,
      newSubId: data.newSubId,
      operationType: direction,
      headers: {
        "PAYMENT-RESPONSE": Buffer.from(
          JSON.stringify({
            newSubId: data.newSubId,
            operationType: direction,
            txHash: data.txHash,
          }),
          "utf8",
        ).toString("base64"),
      },
    };
  }

  // ── SubscriptionCapability: Cancel ─────────────────────────────────────
  async verifyCancel(auth: CancelAuth, subId: string): Promise<VerifyResult> {
    // Only the cross-id check is SDK-owned: the buyer could otherwise present
    // an auth for sub A but POST to /subscriptions/B/cancel — whether the
    // contract enforces auth.subId === URL subId is implementation-specific,
    // so we keep this guard at the SDK layer.
    //
    // Everything else (signature, deadline, state==active) is rejected by
    // the contract on settle.
    if (auth.subId !== subId) {
      // auth.subId != URL subId — buyer is presenting a cancel signature
      // bound to a different sub.
      return { ok: false, error: ErrorCode.CancelSignatureInvalid };
    }
    return { ok: true };
  }

  async settleCancel(auth: CancelAuth, subId: string): Promise<SettleCancelResult> {
    let envelope;
    try {
      envelope = await this.facilitator.cancelSubscription(subId, auth);
    } catch {
      return { success: false, error: ErrorCode.ConfirmationTimeout };
    }

    if (envelope.code !== 0 || !envelope.data) {
      return { success: false, error: envelope.msg ?? ErrorCode.ConfirmationTimeout };
    }

    const sub = await this.store.get(subId);
    if (sub) {
      await this.store.put({ ...sub, state: "canceled" });
    }

    return {
      success: true,
      subId,
      headers: {
        "PAYMENT-RESPONSE": Buffer.from(
          JSON.stringify({ subId, txHash: envelope.data.txHash }),
          "utf8",
        ).toString("base64"),
      },
    };
  }

  // ── SubscriptionCapability: Cancel-Pending-Change ─────────────────────
  async verifyCancelPendingChange(
    auth: PendingChangeCancelAuth,
    subId: string,
  ): Promise<VerifyResult> {
    // SDK owns the cross-id check only; facilitator enforces
    // `auth.newSubId == pendingPlanChange.newSubId`
    // (pending_cancel_target_mismatch) and no_pending_change on state.
    if (auth.subId !== subId) {
      return { ok: false, error: ErrorCode.CancelSignatureInvalid };
    }
    return { ok: true };
  }

  async settleCancelPendingChange(
    auth: PendingChangeCancelAuth,
    subId: string,
  ): Promise<SettleCancelPendingChangeResult> {
    let envelope;
    try {
      envelope = await this.facilitator.cancelPendingChange(subId, auth);
    } catch {
      return { success: false, error: ErrorCode.ConfirmationTimeout };
    }
    if (envelope.code !== 0 || !envelope.data) {
      return { success: false, error: envelope.msg ?? ErrorCode.ConfirmationTimeout };
    }
    // Sub itself stays ACTIVE — only pendingPlanChange is retired. Re-pull
    // GET /detail so store reflects the cleared pending row (and any other
    // fields that may have shifted by then).
    const detail = await this.getDetailOnce(subId);
    if (detail) await this.store.put(this.subscriptionFromGetResp(detail));

    return {
      success: true,
      subId,
      headers: {
        "PAYMENT-RESPONSE": Buffer.from(
          JSON.stringify({ subId, txHash: envelope.data.txHash }),
          "utf8",
        ).toString("base64"),
      },
    };
  }

  async charge(subId: string): Promise<ChargeResult> {
    let envelope;
    try {
      envelope = await this.facilitator.chargeSubscription(subId);
    } catch {
      // Network / HTTP layer failure: we don't know if the on-chain charge
      // happened. Surface as ConfirmationTimeout so Seller can call
      // syncFromChain.
      throw new ChargeError(ChargeErrorCode.ConfirmationTimeout, subId, undefined);
    }

    if (envelope.code !== 0 || !envelope.data) {
      // Facilitator rejected; reason string maps to one of the 6 known codes.
      throw new ChargeError(
        mapToChargeError(envelope.msg ?? undefined),
        subId,
        envelope.data?.txHash,
      );
    }

    const data = envelope.data;

    if (data.planChangeTriggered && data.newSubId) {
      // Downgrade just activated: poll the new sub until its first period is
      // charged, then pull the old sub once to capture its CHANGED state
      // (pendingPlanChange cleared, changedToSubId set on-chain).
      const newDetail = await this.pollUntilSettled(data.newSubId);
      if (!newDetail) {
        throw new ChargeError(ChargeErrorCode.ConfirmationTimeout, data.newSubId, data.txHash);
      }
      await this.store.put(this.subscriptionFromGetResp(newDetail));
      const oldDetail = await this.getDetailOnce(subId);
      if (oldDetail) {
        await this.store.put(this.subscriptionFromGetResp(oldDetail));
      }
    } else {
      // Normal charge: poll until GET /detail shows
      // `lastChargedPeriod >= elapsedPeriods` (i.e. this period's charge has
      // landed). Use the polled snapshot as the new store entry.
      const detail = await this.pollUntilSettled(subId);
      if (!detail) {
        throw new ChargeError(ChargeErrorCode.ConfirmationTimeout, subId, data.txHash);
      }
      await this.store.put(this.subscriptionFromGetResp(detail));
    }

    const sub = await this.store.get(subId);
    const periodAmount = sub?.amountPerPeriod ?? "0";
    return {
      success: true,
      period: data.period,
      amount: periodAmount,
      txHash: data.txHash,
      planChangeTriggered: data.planChangeTriggered,
      newSubId: data.newSubId ?? undefined,
    };
  }

  // ── internals ──────────────────────────────────────────────────────────
  /**
   * Map a facilitator state enum to the SDK store's string state.
   *   0 pending / 1 active / 2 completed / 3 canceled / 4 changed / 99 failed
   */
  protected stateFromNumber(n: number | undefined): SubscriptionState {
    switch (n) {
      case 0:
        return "pending";
      case 2:
        return "completed";
      case 3:
        return "canceled";
      case 4:
        return "changed";
      case 99:
        return "failed";
      default:
        return "active";
    }
  }

  /**
   * Build the canonical Subscription from a GET /subscriptions/detail response.
   * GET is the sole source of truth for store entries.
   */
  protected subscriptionFromGetResp(
    data: import("@okxweb3/app-x402-core/subscription").FacilitatorGetSubscriptionData,
  ): Subscription {
    const pending = data.pendingPlanChange;
    return {
      subId: data.subId,
      payer: data.payer,
      merchant: data.merchant,
      token: data.token,
      amountPerPeriod: data.amountPerPeriod,
      periodMode: data.periodMode,
      periodSec: data.periodSec,
      billingAnchorAt: data.billingAnchorAt,
      maxPeriods: data.maxPeriods,
      startAt: data.startAt,
      state: this.stateFromNumber(data.state),
      lastChargedPeriod: data.lastChargedPeriod,
      totalPulled: data.totalPulled,
      // GET /detail returns planId(String) and planTier(Integer) directly.
      planId: data.planId ?? "",
      planTier: data.planTier ?? 0,
      changedToSubId: data.changedToSubId ?? undefined,
      isActive: data.isActive,
      serviceEnded: data.serviceEnded,
      currentPeriod: data.currentPeriod,
      elapsedPeriods: data.elapsedPeriods,
      nextChargeableAt: data.nextChargeableAt,
      pendingPlanChange: pending
        ? {
            subId: pending.subId,
            newSubId: pending.newSubId,
            effectiveFromPeriod: pending.effectiveFromPeriod,
            state: pending.state,
          }
        : undefined,
    };
  }

  /**
   * Client-side settlement polling: GET /subscriptions/detail up to 5 times,
   * first attempt immediate and subsequent attempts at 1s intervals (5s max
   * total). Returns the raw GET data once
   * `lastChargedPeriod >= elapsedPeriods` (`elapsedPeriods` is the real,
   * NOT-clamped period count; equality means the current elapsed period has
   * been charged). Used by `settleSubscribe`, `settleChange` (upgrade), and
   * `charge` to align store with the facilitator's authoritative view.
   *
   * Returns null when all 5 attempts time out — caller surfaces a
   * pending-with-subId error so the seller can sync later.
   */
  protected async pollUntilSettled(
    subId: string,
  ): Promise<import("@okxweb3/app-x402-core/subscription").FacilitatorGetSubscriptionData | null> {
    const sleep = (globalThis as unknown as { setTimeout: (cb: () => void, ms: number) => unknown })
      .setTimeout;
    for (let i = 0; i < 5; i++) {
      if (i > 0) await new Promise<void>(r => sleep(() => r(), 1000));
      let envelope;
      try {
        envelope = await this.facilitator.getSubscription(subId);
      } catch {
        continue;
      }
      if (envelope.code !== 0 || !envelope.data) continue;
      const d = envelope.data;
      // Settled iff lastChargedPeriod has caught up to elapsedPeriods, with
      // a guard against a half-recorded write where elapsedPeriods>=1 but
      // first-period charge hasn't landed yet. The pre-start case
      // (elapsedPeriods===0, lastChargedPeriod===0) is legitimately settled
      // — the subscription exists but won't charge until startAt elapses.
      if (
        d.elapsedPeriods != null &&
        d.lastChargedPeriod >= d.elapsedPeriods &&
        (d.elapsedPeriods === 0 || d.lastChargedPeriod > 0)
      ) {
        return d;
      }
    }
    return null;
  }

  /**
   * Single GET /detail (no polling). Used by `settleChange` downgrade and
   * `charge` planChange paths to pull the authoritative pendingPlanChange /
   * state of a sub that doesn't need to wait for first-period charge.
   */
  protected async getDetailOnce(
    subId: string,
  ): Promise<import("@okxweb3/app-x402-core/subscription").FacilitatorGetSubscriptionData | null> {
    let envelope;
    try {
      envelope = await this.facilitator.getSubscription(subId);
    } catch {
      return null;
    }
    if (envelope.code !== 0 || !envelope.data) return null;
    return envelope.data;
  }
}
