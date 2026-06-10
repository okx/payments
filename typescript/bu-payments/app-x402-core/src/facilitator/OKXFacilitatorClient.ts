import crypto from "node:crypto";
import type { FacilitatorClient } from "../http/httpFacilitatorClient.js";
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

  /**
   *
   * @param config
   */
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
}
