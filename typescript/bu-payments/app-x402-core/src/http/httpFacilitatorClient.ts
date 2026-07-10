import { PaymentPayload, PaymentRequirements } from "../types/payments";
import {
  VerifyResponse,
  SettleResponse,
  SettleStatusResponse,
  SupportedResponse,
  VerifyError,
  SettleError,
  FacilitatorResponseError,
} from "../types/facilitator";
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
} from "../subscription/facilitator-client";
import { parseChainIdFromNetwork } from "../subscription/codec/typed-data";
import { asSubscriptionPaymentInner } from "../subscription/codec/payload";
import type { CancelAuth, PendingChangeCancelAuth } from "../subscription/types";
import { z } from "../schemas";

const DEFAULT_FACILITATOR_URL = "https://web3.okx.com/facilitator";

export interface FacilitatorConfig {
  url?: string;
  createAuthHeaders?: () => Promise<{
    verify: Record<string, string>;
    settle: Record<string, string>;
    supported: Record<string, string>;
  }>;
  /**
   * Optional per-operation auth header generator for subscription endpoints.
   * Called with `subscribe` | `change` | `cancel` | `charge` | `getSubscription`.
   * If omitted, subscription requests go out without auth headers (suitable
   * for self-hosted facilitators or tests; for OKX use `OKXFacilitatorClient`).
   */
  createSubscriptionAuthHeaders?: (op: string) => Promise<Record<string, string>>;
  /**
   * Inject a fetch implementation (test mock / custom transport). Defaults
   * to the global `fetch`.
   */
  fetchFn?: typeof fetch;
}

/**
 * Interface for facilitator clients
 * Can be implemented for HTTP-based or local facilitators
 */
export interface FacilitatorClient {
  /**
   * Verify a payment with the facilitator
   *
   * @param paymentPayload - The payment to verify
   * @param paymentRequirements - The requirements to verify against
   * @returns Verification response
   */
  verify(
    paymentPayload: PaymentPayload,
    paymentRequirements: PaymentRequirements,
  ): Promise<VerifyResponse>;

  /**
   * Settle a payment with the facilitator
   *
   * @param paymentPayload - The payment to settle
   * @param paymentRequirements - The requirements for settlement
   * @returns Settlement response
   */
  settle(
    paymentPayload: PaymentPayload,
    paymentRequirements: PaymentRequirements,
  ): Promise<SettleResponse>;

  /**
   * Get supported payment kinds and extensions from the facilitator
   *
   * @returns Supported payment kinds and extensions
   */
  getSupported(): Promise<SupportedResponse>;

  /**
   * Query on-chain settlement status by transaction hash.
   * OKX extension: used for timeout recovery polling.
   *
   * @param txHash - The transaction hash to query
   * @returns Settlement status response
   */
  getSettleStatus?(txHash: string): Promise<SettleStatusResponse>;

  /**
   * Verify only the payment signature, without any settleability checks
   * (no blacklist / KYS / parameter match / time window / on-chain / anti-replay).
   * OKX extension used by the settlement-exemption fast path: a valid result
   * confirms the payload was signed by its declared payer, NOT that it is safe
   * to settle. `paymentRequirements` does not participate in verification and
   * may be omitted.
   *
   * @param paymentPayload - The payment payload carrying the signature
   * @param paymentRequirements - Optional requirements (unused for verification)
   * @returns Signature verification response
   */
  verifySignature?(
    paymentPayload: PaymentPayload,
    paymentRequirements?: PaymentRequirements,
  ): Promise<VerifyResponse>;
}

/** Number of retries for getSupported() on 429 rate limit errors */
const GET_SUPPORTED_RETRIES = 3;
/** Base delay in ms for exponential backoff on retries */
const GET_SUPPORTED_RETRY_DELAY_MS = 1000;

const verifyResponseSchema: z.ZodType<VerifyResponse, z.ZodTypeDef, unknown> = z.object({
  isValid: z.boolean(),
  invalidReason: z
    .string()
    .nullish()
    .transform(v => v ?? undefined),
  invalidMessage: z
    .string()
    .nullish()
    .transform(v => v ?? undefined),
  payer: z
    .string()
    .nullish()
    .transform(v => v ?? undefined),
  extensions: z
    .record(z.string(), z.unknown())
    .nullish()
    .transform(v => v ?? undefined),
});

const settleResponseSchema: z.ZodType<SettleResponse, z.ZodTypeDef, unknown> = z.object({
  success: z.boolean(),
  // OKX extension: pending (async), success (immediate), timeout (on-chain timed out)
  status: z
    .enum(["pending", "success", "timeout"])
    .nullish()
    .transform(v => v ?? undefined)
    .optional(),
  errorReason: z
    .string()
    .nullish()
    .transform(v => v ?? undefined),
  errorMessage: z
    .string()
    .nullish()
    .transform(v => v ?? undefined),
  payer: z
    .string()
    .nullish()
    .transform(v => v ?? undefined),
  transaction: z.string(),
  network: z.custom<SettleResponse["network"]>(value => typeof value === "string"),
  amount: z
    .string()
    .nullish()
    .transform(v => v ?? undefined),
  extensions: z
    .record(z.string(), z.unknown())
    .nullish()
    .transform(v => v ?? undefined),
});

const supportedKindSchema: z.ZodType<SupportedResponse["kinds"][number], z.ZodTypeDef, unknown> =
  z.object({
    x402Version: z.number(),
    scheme: z.string(),
    network: z.custom<SupportedResponse["kinds"][number]["network"]>(
      value => typeof value === "string",
    ),
    extra: z
      .record(z.string(), z.unknown())
      .nullish()
      .transform(v => v ?? undefined),
  });

const supportedResponseSchema: z.ZodType<SupportedResponse, z.ZodTypeDef, unknown> = z.object({
  kinds: z.array(supportedKindSchema),
  extensions: z.array(z.string()).default([]),
  signers: z.record(z.string(), z.array(z.string())).default({}),
});

/**
 * Produces a compact excerpt of a facilitator response body for error messages.
 *
 * @param text - The raw response body text
 * @param limit - The maximum number of characters to include
 * @returns A normalized excerpt suitable for logs and thrown errors
 */
function responseExcerpt(text: string, limit: number = 200): string {
  const compact = text.trim().replace(/\s+/g, " ");
  if (!compact) {
    return "<empty response>";
  }

  if (compact.length <= limit) {
    return compact;
  }

  return `${compact.slice(0, limit - 3)}...`;
}

/**
 * Parses and validates a successful facilitator response body.
 *
 * @param response - The HTTP response returned by the facilitator
 * @param schema - The schema used to validate the response payload
 * @param operation - The facilitator operation name for error reporting
 * @returns The validated facilitator payload
 */
async function parseSuccessResponse<T>(
  response: Response,
  schema: z.ZodType<T, z.ZodTypeDef, unknown>,
  operation: string,
): Promise<T> {
  const text = await response.text();

  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    throw new FacilitatorResponseError(
      `Facilitator ${operation} returned invalid JSON: ${responseExcerpt(text)}`,
    );
  }

  const parsed = schema.safeParse(data);
  if (!parsed.success) {
    throw new FacilitatorResponseError(
      `Facilitator ${operation} returned invalid data: ${responseExcerpt(text)}`,
    );
  }

  return parsed.data;
}

/**
 * HTTP-based client for interacting with x402 facilitator services
 * Handles HTTP communication with facilitator endpoints
 */
export class HTTPFacilitatorClient implements FacilitatorClient {
  readonly url: string;
  private readonly _createAuthHeaders?: FacilitatorConfig["createAuthHeaders"];
  private readonly _createSubscriptionAuthHeaders?: FacilitatorConfig["createSubscriptionAuthHeaders"];
  private readonly _fetchFn: typeof fetch;

  /**
   * Creates a new HTTPFacilitatorClient instance.
   *
   * @param config - Configuration options for the facilitator client
   */
  constructor(config?: FacilitatorConfig) {
    this.url = config?.url || DEFAULT_FACILITATOR_URL;
    this._createAuthHeaders = config?.createAuthHeaders;
    this._createSubscriptionAuthHeaders = config?.createSubscriptionAuthHeaders;
    this._fetchFn = config?.fetchFn ?? fetch;
  }

  /**
   * Verify a payment with the facilitator
   *
   * @param paymentPayload - The payment to verify
   * @param paymentRequirements - The requirements to verify against
   * @returns Verification response
   */
  async verify(
    paymentPayload: PaymentPayload,
    paymentRequirements: PaymentRequirements,
  ): Promise<VerifyResponse> {
    let headers: Record<string, string> = {
      "Content-Type": "application/json",
    };

    if (this._createAuthHeaders) {
      const authHeaders = await this.createAuthHeaders("verify");
      headers = { ...headers, ...authHeaders.headers };
    }

    const response = await this._fetchFn(`${this.url}/verify`, {
      method: "POST",
      headers,
      body: JSON.stringify({
        x402Version: paymentPayload.x402Version,
        paymentPayload: this.toJsonSafe(paymentPayload),
        paymentRequirements: this.toJsonSafe(paymentRequirements),
      }),
    });

    if (!response.ok) {
      const text = await response.text();
      let data: unknown;
      try {
        data = JSON.parse(text);
      } catch {
        throw new Error(`Facilitator verify failed (${response.status}): ${responseExcerpt(text)}`);
      }

      if (typeof data === "object" && data !== null && "isValid" in data) {
        throw new VerifyError(response.status, data as VerifyResponse);
      }

      throw new Error(
        `Facilitator verify failed (${response.status}): ${responseExcerpt(JSON.stringify(data))}`,
      );
    }

    return parseSuccessResponse(response, verifyResponseSchema, "verify");
  }

  /**
   * Settle a payment with the facilitator
   *
   * @param paymentPayload - The payment to settle
   * @param paymentRequirements - The requirements for settlement
   * @returns Settlement response
   */
  async settle(
    paymentPayload: PaymentPayload,
    paymentRequirements: PaymentRequirements,
  ): Promise<SettleResponse> {
    let headers: Record<string, string> = {
      "Content-Type": "application/json",
    };

    if (this._createAuthHeaders) {
      const authHeaders = await this.createAuthHeaders("settle");
      headers = { ...headers, ...authHeaders.headers };
    }

    const response = await this._fetchFn(`${this.url}/settle`, {
      method: "POST",
      headers,
      body: JSON.stringify({
        x402Version: paymentPayload.x402Version,
        paymentPayload: this.toJsonSafe(paymentPayload),
        paymentRequirements: this.toJsonSafe(paymentRequirements),
      }),
    });

    if (!response.ok) {
      const text = await response.text();
      let data: unknown;
      try {
        data = JSON.parse(text);
      } catch {
        throw new Error(`Facilitator settle failed (${response.status}): ${responseExcerpt(text)}`);
      }

      if (typeof data === "object" && data !== null && "success" in data) {
        throw new SettleError(response.status, data as SettleResponse);
      }

      throw new Error(
        `Facilitator settle failed (${response.status}): ${responseExcerpt(JSON.stringify(data))}`,
      );
    }

    return parseSuccessResponse(response, settleResponseSchema, "settle");
  }

  /**
   * Get supported payment kinds and extensions from the facilitator.
   * Retries with exponential backoff on 429 rate limit errors.
   *
   * @returns Supported payment kinds and extensions
   */
  async getSupported(): Promise<SupportedResponse> {
    let headers: Record<string, string> = {
      "Content-Type": "application/json",
    };

    if (this._createAuthHeaders) {
      const authHeaders = await this.createAuthHeaders("supported");
      headers = { ...headers, ...authHeaders.headers };
    }

    let lastError: Error | null = null;
    for (let attempt = 0; attempt < GET_SUPPORTED_RETRIES; attempt++) {
      const response = await this._fetchFn(`${this.url}/supported`, {
        method: "GET",
        headers,
      });

      if (response.ok) {
        return parseSuccessResponse(response, supportedResponseSchema, "supported");
      }

      const errorText = await response.text().catch(() => response.statusText);
      lastError = new Error(
        `Facilitator getSupported failed (${response.status}): ${responseExcerpt(errorText)}`,
      );

      // Retry on 429 rate limit errors with exponential backoff
      if (response.status === 429 && attempt < GET_SUPPORTED_RETRIES - 1) {
        const delay = GET_SUPPORTED_RETRY_DELAY_MS * Math.pow(2, attempt);
        await new Promise(resolve => setTimeout(resolve, delay));
        continue;
      }

      throw lastError;
    }

    throw lastError ?? new Error("Facilitator getSupported failed after retries");
  }

  /**
   * Query on-chain settlement status by transaction hash.
   *
   * @param txHash - The transaction hash to query
   * @returns Settlement status response
   */
  async getSettleStatus(txHash: string): Promise<SettleStatusResponse> {
    let headers: Record<string, string> = {
      "Content-Type": "application/json",
    };

    if (this._createAuthHeaders) {
      const authHeaders = await this.createAuthHeaders("settle/status");
      headers = { ...headers, ...authHeaders.headers };
    }

    const response = await this._fetchFn(
      `${this.url}/settle/status?txHash=${encodeURIComponent(txHash)}`,
      {
        method: "GET",
        headers,
      },
    );

    if (!response.ok) {
      const text = await response.text().catch(() => response.statusText);
      throw new Error(
        `Facilitator getSettleStatus failed (${response.status}): ${responseExcerpt(text)}`,
      );
    }

    const json = (await response.json()) as SettleStatusResponse;
    return json;
  }

  /**
   * Creates authentication headers for a specific path.
   *
   * @param path - The path to create authentication headers for (e.g., "verify", "settle", "supported")
   * @returns An object containing the authentication headers for the specified path
   */
  async createAuthHeaders(path: string): Promise<{
    headers: Record<string, string>;
  }> {
    if (this._createAuthHeaders) {
      const authHeaders = (await this._createAuthHeaders()) as Record<
        string,
        Record<string, string>
      >;
      return {
        headers: authHeaders[path] ?? {},
      };
    }
    return {
      headers: {},
    };
  }

  /**
   * Helper to convert objects to JSON-safe format.
   * Handles BigInt and other non-JSON types.
   *
   * @param obj - The object to convert
   * @returns The JSON-safe representation of the object
   */
  private toJsonSafe(obj: unknown): unknown {
    return JSON.parse(
      JSON.stringify(obj, (_, value) => (typeof value === "bigint" ? value.toString() : value)),
    );
  }

  // ── SubscriptionFacilitatorClient (period) ─────────────
  //
  // Generic JSON POST / GET helpers parameterized by `op` so the same code
  // path covers all five subscription endpoints. The standard OKX envelope
  // `{ code, msg?, data? }` is returned to the caller unparsed (the
  // subscription scheme reads `code === "0"` and `data` directly).

  private async subscriptionAuthHeaders(op: string): Promise<Record<string, string>> {
    if (!this._createSubscriptionAuthHeaders) return {};
    return this._createSubscriptionAuthHeaders(op);
  }

  private async subscriptionPost<T>(
    op: string,
    path: string,
    body: unknown,
  ): Promise<FacilitatorEnvelope<T>> {
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
      ...(await this.subscriptionAuthHeaders(op)),
    };
    const resp = await this._fetchFn(`${this.url}${path}`, {
      method: "POST",
      headers,
      body: JSON.stringify(this.toJsonSafe(body)),
    });
    if (!resp.ok) {
      throw new Error(`facilitator ${op} returned HTTP ${resp.status}: ${await resp.text()}`);
    }
    return (await resp.json()) as FacilitatorEnvelope<T>;
  }

  private async subscriptionGet<T>(op: string, path: string): Promise<FacilitatorEnvelope<T>> {
    const headers = await this.subscriptionAuthHeaders(op);
    const resp = await this._fetchFn(`${this.url}${path}`, { method: "GET", headers });
    if (!resp.ok) {
      throw new Error(`facilitator ${op} returned HTTP ${resp.status}: ${await resp.text()}`);
    }
    return (await resp.json()) as FacilitatorEnvelope<T>;
  }

  /**
   * Build the {chainIndex, terms, permit, termsSig, permitSig, syncSettle}
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
    return this.subscriptionPost<FacilitatorSubscribeData>(
      "subscribe",
      "/api/v6/pay/x402/subscriptions",
      this.buildWriteBody(paymentPayload, paymentRequirements, syncSettle),
    );
  }

  async changeSubscription(
    paymentPayload: PaymentPayload,
    paymentRequirements: PaymentRequirements,
    oldSubId: string,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorChangeData>> {
    return this.subscriptionPost<FacilitatorChangeData>(
      "change",
      "/api/v6/pay/x402/subscriptions/change",
      {
        ...this.buildWriteBody(paymentPayload, paymentRequirements, syncSettle),
        // `oldSubId` is informational — server reads
        // newTerms.changeFromSubId for the authoritative value.
        oldSubId,
        // change body uses `newTerms` not `terms`.
        newTerms: asSubscriptionPaymentInner(paymentPayload).terms,
        terms: undefined,
      },
    );
  }

  async cancelSubscription(
    subId: string,
    cancelAuth: CancelAuth,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorCancelData>> {
    return this.subscriptionPost<FacilitatorCancelData>(
      "cancel",
      "/api/v6/pay/x402/subscriptions/cancel",
      { subId, cancelAuth, syncSettle: syncSettle ?? true },
    );
  }

  async cancelPendingChange(
    subId: string,
    cancelAuth: PendingChangeCancelAuth,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorCancelPendingData>> {
    return this.subscriptionPost<FacilitatorCancelPendingData>(
      "cancel-pending-change",
      "/api/v6/pay/x402/subscriptions/cancel-pending-change",
      { subId, cancelAuth, syncSettle: syncSettle ?? true },
    );
  }

  async chargeSubscription(
    subId: string,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorChargeData>> {
    return this.subscriptionPost<FacilitatorChargeData>(
      "charge",
      "/api/v6/pay/x402/subscriptions/charge",
      { subId, syncSettle: syncSettle ?? true },
    );
  }

  async finalizeExpired(
    subId: string,
    syncSettle?: boolean,
  ): Promise<FacilitatorEnvelope<FacilitatorFinalizeExpiredData>> {
    return this.subscriptionPost<FacilitatorFinalizeExpiredData>(
      "finalize-expired",
      "/api/v6/pay/x402/subscriptions/finalize-expired",
      { subId, syncSettle: syncSettle ?? true },
    );
  }

  async getCharges(
    subId: string,
    limit = 50,
    offset = 0,
  ): Promise<FacilitatorEnvelope<FacilitatorGetChargesData>> {
    const q = new URLSearchParams({ subId, limit: String(limit), offset: String(offset) });
    return this.subscriptionGet<FacilitatorGetChargesData>(
      "getCharges",
      `/api/v6/pay/x402/subscriptions/charges?${q.toString()}`,
    );
  }

  async getPendingChange(
    subId: string,
  ): Promise<FacilitatorEnvelope<FacilitatorPendingChangeRow | null>> {
    return this.subscriptionGet<FacilitatorPendingChangeRow | null>(
      "getPendingChange",
      `/api/v6/pay/x402/subscriptions/pending?subId=${encodeURIComponent(subId)}`,
    );
  }

  async getSubscription(
    subId: string,
  ): Promise<FacilitatorEnvelope<FacilitatorGetSubscriptionData>> {
    return this.subscriptionGet<FacilitatorGetSubscriptionData>(
      "getSubscription",
      `/api/v6/pay/x402/subscriptions/detail?subId=${encodeURIComponent(subId)}`,
    );
  }
}
