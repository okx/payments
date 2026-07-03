import crypto from "node:crypto";
import type { FacilitatorClient } from "../http/httpFacilitatorClient.js";
import type {
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
} from "../subscription/facilitator-client.js";
import { parseChainIdFromNetwork } from "../subscription/codec/typed-data.js";
import { asSubscriptionPaymentInner } from "../subscription/codec/payload.js";
import type { CancelAuth, PendingChangeCancelAuth } from "../subscription/types.js";
import type {
  VerifyResponse,
  SettleResponse,
  SettleStatusResponse,
  SupportedResponse,
} from "../types/facilitator.js";
import type { PaymentPayload, PaymentRequirements } from "../types/payments.js";

export interface OKXConfig {
  apiKey: string;
  secretKey: string;
  passphrase: string;
  baseUrl?: string;
  /**
   * OKX exact-scheme extension: when true, the settle call tells the facilitator to
   * wait for on-chain confirmation before responding (syncSettle=true in request body).
   * The facilitator then returns status="success" directly (no polling needed).
   * When false (default), the facilitator responds with status="pending" immediately.
   */
  syncSettle?: boolean;
}

/**
 * OKX facilitator client implementing the FacilitatorClient interface.
 * Uses HMAC-SHA256 signing per OKX REST API authentication spec.
 */
export class OKXFacilitatorClient implements FacilitatorClient {
  private config: Required<Omit<OKXConfig, "syncSettle">> & Pick<OKXConfig, "syncSettle">;

  constructor(config: OKXConfig) {
    this.config = {
      baseUrl: "https://web3.okx.com",
      ...config,
    };
  }

  /**
   *
   * @param method
   * @param path
   * @param body
   */
  private createHeaders(method: string, path: string, body?: string): Record<string, string> {
    const timestamp = new Date().toISOString();
    const prehash = timestamp + method + path + (body ?? "");
    const sign = crypto
      .createHmac("sha256", this.config.secretKey)
      .update(prehash)
      .digest("base64");

    return {
      "OK-ACCESS-KEY": this.config.apiKey,
      "OK-ACCESS-SIGN": sign,
      "OK-ACCESS-TIMESTAMP": timestamp,
      "OK-ACCESS-PASSPHRASE": this.config.passphrase,
      "Content-Type": "application/json",
    };
  }

  /**
   *
   */
  async getSupported(): Promise<SupportedResponse> {
    const path = "/api/v6/pay/x402/supported";
    const res = await fetch(this.config.baseUrl + path, {
      headers: this.createHeaders("GET", path),
    });
    if (!res.ok) throw new Error(`OKX getSupported failed: ${res.status}`);
    const json = (await res.json()) as Record<string, unknown>;
    // OKX API wraps responses in { code, data, msg } envelope
    const data = (json.data ?? json) as SupportedResponse;
    return data;
  }

  /**
   *
   * @param payload
   * @param requirements
   */
  async verify(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<VerifyResponse> {
    const path = "/api/v6/pay/x402/verify";
    const body = JSON.stringify({
      x402Version: 2,
      paymentPayload: payload,
      paymentRequirements: requirements,
    });
    const res = await fetch(this.config.baseUrl + path, {
      method: "POST",
      headers: this.createHeaders("POST", path, body),
      body,
    });
    if (!res.ok) throw new Error(`OKX verify failed: ${res.status}`);
    const json = (await res.json()) as Record<string, unknown>;
    const data = (json.data ?? json) as VerifyResponse;
    return data;
  }

  /**
   *
   * @param payload
   * @param requirements
   */
  async settle(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
  ): Promise<SettleResponse> {
    const path = "/api/v6/pay/x402/settle";
    // Include OKX-extension syncSettle field when configured:
    //   syncSettle=true  → facilitator waits for on-chain confirmation, returns status="success"
    //   syncSettle=false → facilitator returns immediately with status="pending" (default)
    const bodyObj: Record<string, unknown> = {
      x402Version: 2,
      paymentPayload: payload,
      paymentRequirements: requirements,
    };
    if (this.config.syncSettle !== undefined) {
      bodyObj.syncSettle = this.config.syncSettle;
    }
    const body = JSON.stringify(bodyObj);
    const res = await fetch(this.config.baseUrl + path, {
      method: "POST",
      headers: this.createHeaders("POST", path, body),
      body,
    });
    if (!res.ok) throw new Error(`OKX settle failed: ${res.status}`);
    const json = (await res.json()) as Record<string, unknown>;
    const data = (json.data ?? json) as SettleResponse;
    return data;
  }

  /**
   * Query on-chain settlement status by transaction hash.
   *
   * @param txHash - The transaction hash to query
   * @returns Settlement status response
   */
  async getSettleStatus(txHash: string): Promise<SettleStatusResponse> {
    const path = `/api/v6/pay/x402/settle/status?txHash=${encodeURIComponent(txHash)}`;
    const res = await fetch(this.config.baseUrl + path, {
      headers: this.createHeaders("GET", path),
    });
    if (!res.ok) throw new Error(`OKX getSettleStatus failed: ${res.status}`);
    const json = (await res.json()) as Record<string, unknown>;
    const data = (json.data ?? json) as SettleStatusResponse;
    return data;
  }

  // ── SubscriptionFacilitatorClient (period) ─────────────
  //
  // Subscription endpoints share OKX's HMAC-SHA256 auth (createHeaders) so
  // production sellers route ALL facilitator HTTP through this single
  // client — exact/upto/aggr_deferred go via verify/settle, period
  // goes via these five methods.

  /**
   * Build `{chainIndex, terms, permit, termsSig, permitSig, syncSettle}` —
   * request body shared by subscribe / change endpoints.
   */
  private buildWriteBody(
    payload: PaymentPayload,
    requirements: PaymentRequirements,
    syncSettle?: boolean,
  ): Record<string, unknown> {
    const inner = asSubscriptionPaymentInner(payload);
    return {
      chainIndex: parseChainIdFromNetwork(requirements.network),
      terms: inner.terms,
      permit: inner.permitSingle,
      termsSig: inner.termsSignature,
      permitSig: inner.permitSingleSignature,
      syncSettle: syncSettle ?? true,
    };
  }

  async subscribe(
    paymentPayload: PaymentPayload,
    paymentRequirements: PaymentRequirements,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorSubscribeData>> {
    const path = "/api/v6/pay/x402/subscriptions";
    const body = JSON.stringify(
      this.buildWriteBody(paymentPayload, paymentRequirements, syncSettle),
    );
    const res = await fetch(this.config.baseUrl + path, {
      method: "POST",
      headers: this.createHeaders("POST", path, body),
      body,
    });
    if (!res.ok) throw new Error(`OKX subscribe failed: ${res.status}`);
    return (await res.json()) as FacilitatorEnvelope<FacilitatorSubscribeData>;
  }

  async changeSubscription(
    paymentPayload: PaymentPayload,
    paymentRequirements: PaymentRequirements,
    oldSubId: string,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorChangeData>> {
    const path = "/api/v6/pay/x402/subscriptions/change";
    const inner = asSubscriptionPaymentInner(paymentPayload);
    const body = JSON.stringify({
      chainIndex: parseChainIdFromNetwork(paymentRequirements.network),
      oldSubId,
      newTerms: inner.terms,
      permit: inner.permitSingle,
      termsSig: inner.termsSignature,
      permitSig: inner.permitSingleSignature,
      syncSettle: syncSettle ?? true,
    });
    const res = await fetch(this.config.baseUrl + path, {
      method: "POST",
      headers: this.createHeaders("POST", path, body),
      body,
    });
    if (!res.ok) throw new Error(`OKX changeSubscription failed: ${res.status}`);
    return (await res.json()) as FacilitatorEnvelope<FacilitatorChangeData>;
  }

  async cancelSubscription(
    subId: string,
    cancelAuth: CancelAuth,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorCancelData>> {
    const path = "/api/v6/pay/x402/subscriptions/cancel";
    const body = JSON.stringify({ subId, cancelAuth, syncSettle: syncSettle ?? true });
    const res = await fetch(this.config.baseUrl + path, {
      method: "POST",
      headers: this.createHeaders("POST", path, body),
      body,
    });
    if (!res.ok) throw new Error(`OKX cancelSubscription failed: ${res.status}`);
    return (await res.json()) as FacilitatorEnvelope<FacilitatorCancelData>;
  }

  async cancelPendingChange(
    subId: string,
    cancelAuth: PendingChangeCancelAuth,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorCancelPendingData>> {
    const path = "/api/v6/pay/x402/subscriptions/cancel-pending-change";
    const body = JSON.stringify({ subId, cancelAuth, syncSettle: syncSettle ?? true });
    const res = await fetch(this.config.baseUrl + path, {
      method: "POST",
      headers: this.createHeaders("POST", path, body),
      body,
    });
    if (!res.ok) throw new Error(`OKX cancelPendingChange failed: ${res.status}`);
    return (await res.json()) as FacilitatorEnvelope<FacilitatorCancelPendingData>;
  }

  async chargeSubscription(
    subId: string,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorChargeData>> {
    const path = "/api/v6/pay/x402/subscriptions/charge";
    const body = JSON.stringify({ subId, syncSettle: syncSettle ?? true });
    const res = await fetch(this.config.baseUrl + path, {
      method: "POST",
      headers: this.createHeaders("POST", path, body),
      body,
    });
    if (!res.ok) {
      const errBody = await res.text().catch(() => "");
      throw new Error(`OKX chargeSubscription failed: ${res.status} ${errBody}`);
    }
    return (await res.json()) as FacilitatorEnvelope<FacilitatorChargeData>;
  }

  async finalizeExpired(
    subId: string,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorFinalizeExpiredData>> {
    const path = "/api/v6/pay/x402/subscriptions/finalize-expired";
    const body = JSON.stringify({ subId, syncSettle: syncSettle ?? true });
    const res = await fetch(this.config.baseUrl + path, {
      method: "POST",
      headers: this.createHeaders("POST", path, body),
      body,
    });
    if (!res.ok) {
      const errBody = await res.text().catch(() => "");
      throw new Error(`OKX finalizeExpired failed: ${res.status} ${errBody}`);
    }
    return (await res.json()) as FacilitatorEnvelope<FacilitatorFinalizeExpiredData>;
  }

  async getCharges(
    subId: string,
    limit = 50,
    offset = 0,
  ): Promise<FacilitatorEnvelope<FacilitatorGetChargesData>> {
    const q = new URLSearchParams({ subId, limit: String(limit), offset: String(offset) });
    const path = `/api/v6/pay/x402/subscriptions/charges?${q.toString()}`;
    const res = await fetch(this.config.baseUrl + path, {
      headers: this.createHeaders("GET", path),
    });
    if (!res.ok) {
      const errBody = await res.text().catch(() => "");
      throw new Error(`OKX getCharges failed: ${res.status} ${errBody}`);
    }
    return (await res.json()) as FacilitatorEnvelope<FacilitatorGetChargesData>;
  }

  async getPendingChange(
    subId: string,
  ): Promise<FacilitatorEnvelope<FacilitatorPendingChangeRow | null>> {
    const path = `/api/v6/pay/x402/subscriptions/pending?subId=${encodeURIComponent(subId)}`;
    const res = await fetch(this.config.baseUrl + path, {
      headers: this.createHeaders("GET", path),
    });
    if (!res.ok) {
      const errBody = await res.text().catch(() => "");
      throw new Error(`OKX getPendingChange failed: ${res.status} ${errBody}`);
    }
    return (await res.json()) as FacilitatorEnvelope<FacilitatorPendingChangeRow | null>;
  }

  async getSubscription(
    subId: string,
  ): Promise<FacilitatorEnvelope<FacilitatorGetSubscriptionData>> {
    const path = `/api/v6/pay/x402/subscriptions/detail?subId=${encodeURIComponent(subId)}`;
    const res = await fetch(this.config.baseUrl + path, {
      headers: this.createHeaders("GET", path),
    });
    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(`OKX getSubscription failed: ${res.status} ${body}`);
    }
    return (await res.json()) as FacilitatorEnvelope<FacilitatorGetSubscriptionData>;
  }
}
