/**
 * Logging wrapper for `OKXFacilitatorClient`. Prints every facilitator-bound
 * call's request shape, response envelope (code / msg / data summary), and
 * elapsed time. Useful when driving real-network verification end-to-end —
 * lets you see exactly what the SDK sends and what the facilitator returns
 * without spelunking into SDK internals.
 *
 * Drop-in: pass `wrapFacilitator(realClient)` wherever the scheme expects a
 * `SubscriptionFacilitatorClient`.
 */
/* eslint-disable no-console */
import type {
  PaymentPayload,
  PaymentRequirements,
  VerifyResponse,
  SettleResponse,
  SettleStatusResponse,
  SupportedResponse,
} from "@okxweb3/app-x402-core/types";
import type { OKXFacilitatorClient } from "@okxweb3/app-x402-core";
import type {
  CancelAuth,
  FacilitatorCancelData,
  FacilitatorCancelPendingData,
  FacilitatorChangeData,
  FacilitatorChargeData,
  FacilitatorEnvelope,
  FacilitatorFinalizeExpiredData,
  FacilitatorGetChargesData,
  FacilitatorGetSubscriptionData,
  FacilitatorPendingChangeRow,
  FacilitatorSubscribeData,
  PendingChangeCancelAuth,
  SubscriptionFacilitatorClient,
} from "@okxweb3/app-x402-core/subscription";

/** Compact summary of a terms struct for log readability. */
function summarizeTerms(payload: PaymentPayload): Record<string, unknown> {
  const inner = (payload.payload ?? {}) as {
    terms?: Record<string, unknown>;
    permitSingle?: {
      details?: { token?: string; amount?: string; nonce?: number };
      spender?: string;
    };
  };
  const t = inner.terms ?? {};
  const p = inner.permitSingle ?? {};
  return {
    payer: t.payer,
    planTier: t.planTier,
    amountPerPeriod: t.amountPerPeriod,
    periodSec: t.periodSec,
    periodMode: t.periodMode,
    maxPeriods: t.maxPeriods,
    changeFromSubId: t.changeFromSubId,
    changeEffectiveAt: t.changeEffectiveAt,
    permit_token: p.details?.token,
    permit_amount: p.details?.amount,
    permit_nonce: p.details?.nonce,
    permit_spender: p.spender,
  };
}

/** Render a value short and grep-friendly (truncate long hex). */
function shortHex(v: unknown, head = 10, tail = 6): string {
  if (typeof v !== "string") return String(v);
  if (v.length <= head + tail + 3) return v;
  return `${v.slice(0, head)}…${v.slice(-tail)}`;
}

interface LogCallParams<T> {
  op: string;
  req: Record<string, unknown>;
  fn: () => Promise<T>;
  summarizeResp?: (r: T) => Record<string, unknown>;
}

async function logCall<T>({ op, req, fn, summarizeResp }: LogCallParams<T>): Promise<T> {
  const t0 = Date.now();
  console.log(`[facilitator → ${op}]`, JSON.stringify(req));
  try {
    const resp = await fn();
    const elapsed = Date.now() - t0;
    const summary = summarizeResp ? summarizeResp(resp) : { resp };
    console.log(`[facilitator ← ${op}] ${elapsed}ms`, JSON.stringify(summary));
    return resp;
  } catch (err) {
    const elapsed = Date.now() - t0;
    console.log(
      `[facilitator ✗ ${op}] ${elapsed}ms throw:`,
      err instanceof Error ? err.message : String(err),
    );
    throw err;
  }
}

function summarizeEnvelope<T>(label: string) {
  return (env: FacilitatorEnvelope<T>): Record<string, unknown> => ({
    code: env.code,
    msg: env.msg ?? null,
    [label]: env.data ?? null,
  });
}

/**
 * Wrap `OKXFacilitatorClient` (or any SubscriptionFacilitatorClient) with
 * per-call logging. Returned object satisfies both `FacilitatorClient` and
 * `SubscriptionFacilitatorClient`.
 */
export function wrapFacilitator(
  inner: OKXFacilitatorClient,
): OKXFacilitatorClient & SubscriptionFacilitatorClient {
  const wrapped: SubscriptionFacilitatorClient = {
    // ── base FacilitatorClient ───────────────────────────────────────────
    getSupported(): Promise<SupportedResponse> {
      return logCall({
        op: "getSupported",
        req: {},
        fn: () => inner.getSupported(),
        summarizeResp: r => ({
          kindsCount: r.kinds.length,
          subscriptionKind: r.kinds.find(k => k.scheme === "period") ?? null,
          signers: r.signers ?? {},
        }),
      });
    },
    verify(payload: PaymentPayload, requirements: PaymentRequirements): Promise<VerifyResponse> {
      return logCall({
        op: "verify",
        req: { network: requirements.network, scheme: requirements.scheme },
        fn: () => inner.verify(payload, requirements),
        summarizeResp: r => ({ isValid: r.isValid, invalidReason: r.invalidReason ?? null }),
      });
    },
    settle(payload: PaymentPayload, requirements: PaymentRequirements): Promise<SettleResponse> {
      return logCall({
        op: "settle",
        req: { network: requirements.network, scheme: requirements.scheme },
        fn: () => inner.settle(payload, requirements),
        summarizeResp: r => ({
          success: r.success,
          status: r.status ?? null,
          transaction: shortHex(r.transaction),
          errorReason: r.errorReason ?? null,
        }),
      });
    },
    getSettleStatus(txHash: string): Promise<SettleStatusResponse> {
      return logCall({
        op: "getSettleStatus",
        req: { txHash: shortHex(txHash) },
        fn: () => inner.getSettleStatus(txHash),
        summarizeResp: r => ({ success: r.success, status: r.status ?? null }),
      });
    },

    // ── subscription endpoints ───────────────────────────────────────────
    subscribe(
      payload: PaymentPayload,
      requirements: PaymentRequirements,
      syncSettle?: boolean,
    ): Promise<FacilitatorEnvelope<FacilitatorSubscribeData>> {
      return logCall({
        op: "subscribe",
        req: {
          chainIndex: requirements.network,
          syncSettle: syncSettle ?? true,
          terms: summarizeTerms(payload),
        },
        fn: () => inner.subscribe(payload, requirements, syncSettle),
        summarizeResp: summarizeEnvelope<FacilitatorSubscribeData>("data"),
      });
    },
    changeSubscription(
      payload: PaymentPayload,
      requirements: PaymentRequirements,
      oldSubId: string,
      syncSettle?: boolean,
    ): Promise<FacilitatorEnvelope<FacilitatorChangeData>> {
      return logCall({
        op: "changeSubscription",
        req: {
          chainIndex: requirements.network,
          oldSubId: shortHex(oldSubId),
          syncSettle: syncSettle ?? true,
          terms: summarizeTerms(payload),
        },
        fn: () => inner.changeSubscription(payload, requirements, oldSubId, syncSettle),
        summarizeResp: summarizeEnvelope<FacilitatorChangeData>("data"),
      });
    },
    cancelSubscription(
      subId: string,
      cancelAuth: CancelAuth,
      syncSettle?: boolean,
    ): Promise<FacilitatorEnvelope<FacilitatorCancelData>> {
      return logCall({
        op: "cancelSubscription",
        req: {
          subId: shortHex(subId),
          syncSettle: syncSettle ?? true,
          cancelAuth: {
            action: cancelAuth.action,
            initiator: cancelAuth.initiator,
            nonce: shortHex(cancelAuth.nonce),
            deadline: cancelAuth.deadline,
            signature: shortHex(cancelAuth.signature),
          },
        },
        fn: () => inner.cancelSubscription(subId, cancelAuth, syncSettle),
        summarizeResp: summarizeEnvelope<FacilitatorCancelData>("data"),
      });
    },
    cancelPendingChange(
      subId: string,
      cancelAuth: PendingChangeCancelAuth,
      syncSettle?: boolean,
    ): Promise<FacilitatorEnvelope<FacilitatorCancelPendingData>> {
      return logCall({
        op: "cancelPendingChange",
        req: { subId: shortHex(subId), syncSettle: syncSettle ?? true },
        fn: () => inner.cancelPendingChange(subId, cancelAuth, syncSettle),
        summarizeResp: summarizeEnvelope<FacilitatorCancelPendingData>("data"),
      });
    },
    chargeSubscription(
      subId: string,
      syncSettle?: boolean,
    ): Promise<FacilitatorEnvelope<FacilitatorChargeData>> {
      return logCall({
        op: "chargeSubscription",
        req: { subId: shortHex(subId), syncSettle: syncSettle ?? true },
        fn: () => inner.chargeSubscription(subId, syncSettle),
        summarizeResp: summarizeEnvelope<FacilitatorChargeData>("data"),
      });
    },
    finalizeExpired(
      subId: string,
      syncSettle?: boolean,
    ): Promise<FacilitatorEnvelope<FacilitatorFinalizeExpiredData>> {
      return logCall({
        op: "finalizeExpired",
        req: { subId: shortHex(subId), syncSettle: syncSettle ?? true },
        fn: () => inner.finalizeExpired(subId, syncSettle),
        summarizeResp: summarizeEnvelope<FacilitatorFinalizeExpiredData>("data"),
      });
    },
    getCharges(
      subId: string,
      limit?: number,
      offset?: number,
    ): Promise<FacilitatorEnvelope<FacilitatorGetChargesData>> {
      return logCall({
        op: "getCharges",
        req: { subId: shortHex(subId), limit, offset },
        fn: () => inner.getCharges(subId, limit, offset),
        summarizeResp: r => ({ code: r.code, count: r.data?.charges?.length ?? 0 }),
      });
    },
    getPendingChange(
      subId: string,
    ): Promise<FacilitatorEnvelope<FacilitatorPendingChangeRow | null>> {
      return logCall({
        op: "getPendingChange",
        req: { subId: shortHex(subId) },
        fn: () => inner.getPendingChange(subId),
        summarizeResp: summarizeEnvelope<FacilitatorPendingChangeRow | null>("data"),
      });
    },
    getSubscription(subId: string): Promise<FacilitatorEnvelope<FacilitatorGetSubscriptionData>> {
      return logCall({
        op: "getSubscription",
        req: { subId: shortHex(subId) },
        fn: () => inner.getSubscription(subId),
        summarizeResp: summarizeEnvelope<FacilitatorGetSubscriptionData>("data"),
      });
    },
  };

  return wrapped as OKXFacilitatorClient & SubscriptionFacilitatorClient;
}
