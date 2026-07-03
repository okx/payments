import { x402ResourceServer, SettlementOverrides, DEFAULT_POLL_DEADLINE_MS } from "../server";
import {
  decodePaymentSignatureHeader,
  encodePaymentRequiredHeader,
  encodePaymentResponseHeader,
} from ".";
import {
  PaymentPayload,
  PaymentRequired,
  SettleResponse,
  SettleError,
  FacilitatorResponseError,
  Price,
  Network,
  PaymentRequirements,
} from "../types";
import { x402Version } from "..";
import type { SubscriptionCapability } from "../subscription";

export const SETTLEMENT_OVERRIDES_HEADER = "settlement-overrides";

/**
 * Framework-agnostic HTTP adapter interface
 * Implementations provide framework-specific HTTP operations
 */
export interface HTTPAdapter {
  getHeader(name: string): string | undefined;
  getMethod(): string;
  getPath(): string;
  getUrl(): string;
  getAcceptHeader(): string;
  getUserAgent(): string;

  /**
   * Return the full request headers as a plain lowercase-keyed record.
   * Optional; adapters that don't implement it cause hooks like
   * `onBeforeAccess` to receive an empty headers map.
   */
  getHeaders?(): Record<string, string>;

  /**
   * Get query parameters from the request URL
   *
   * @returns Record of query parameter key-value pairs
   */
  getQueryParams?(): Record<string, string | string[]>;

  /**
   * Get a specific query parameter by name
   *
   * @param name - The query parameter name
   * @returns The query parameter value(s) or undefined
   */
  getQueryParam?(name: string): string | string[] | undefined;

  /**
   * Get the parsed request body
   * Framework adapters should parse JSON/form data appropriately
   *
   * @returns The parsed request body
   */
  getBody?(): unknown;
}

/**
 * Paywall configuration for HTML responses
 */
export interface PaywallConfig {
  appName?: string;
  appLogo?: string;
  sessionTokenEndpoint?: string;
  currentUrl?: string;
  testnet?: boolean;
}

/**
 * Paywall provider interface for generating HTML
 */
export interface PaywallProvider {
  generateHtml(paymentRequired: PaymentRequired, config?: PaywallConfig): string;
}

/**
 * Dynamic payTo function that receives HTTP request context
 */
export type DynamicPayTo = (context: HTTPRequestContext) => string | Promise<string>;

/**
 * Dynamic price function that receives HTTP request context
 */
export type DynamicPrice = (context: HTTPRequestContext) => Price | Promise<Price>;

/**
 * Result of response body callbacks containing content type and body.
 */
export interface HTTPResponseBody {
  /**
   * The content type for the response (e.g., 'application/json', 'text/plain').
   */
  contentType: string;

  /**
   * The response body to include in the 402 response.
   */
  body: unknown;
}

/**
 * Dynamic function to generate a custom response for unpaid requests.
 * Receives the HTTP request context and returns the content type and body to include in the 402 response.
 */
export type UnpaidResponseBody = (
  context: HTTPRequestContext,
) => HTTPResponseBody | Promise<HTTPResponseBody>;

/**
 * Dynamic function to generate a custom response for settlement failures.
 * Receives the HTTP request context and settle failure result, returns the content type and body.
 */
export type SettlementFailedResponseBody = (
  context: HTTPRequestContext,
  settleResult: Omit<ProcessSettleFailureResponse, "response">,
) => HTTPResponseBody | Promise<HTTPResponseBody>;

/**
 * A single payment option for a route
 * Represents one way a client can pay for access to the resource
 */
export interface PaymentOption {
  scheme: string;
  payTo: string | DynamicPayTo;
  price: Price | DynamicPrice;
  network: Network;
  maxTimeoutSeconds?: number;
  extra?: Record<string, unknown>;
}

/**
 * Route configuration for HTTP endpoints
 *
 * The 'accepts' field defines payment options for the route.
 * Can be a single PaymentOption or an array of PaymentOptions for multiple payment methods.
 */
export interface RouteConfig {
  // Payment option(s): single or array
  accepts: PaymentOption | PaymentOption[];

  // HTTP-specific metadata
  resource?: string;
  description?: string;
  mimeType?: string;
  customPaywallHtml?: string;

  /**
   * Optional callback to generate a custom response for unpaid API requests.
   * This allows servers to return preview data, error messages, or other content
   * when a request lacks payment.
   *
   * For browser requests (Accept: text/html), the paywall HTML takes precedence.
   * This callback is only used for API clients.
   *
   * If not provided, defaults to { contentType: 'application/json', body: {} }.
   *
   * @param context - The HTTP request context
   * @returns An object containing both contentType and body for the 402 response
   */
  unpaidResponseBody?: UnpaidResponseBody;

  /**
   * Optional callback to generate a custom response for settlement failures.
   * If not provided, defaults to { contentType: 'application/json', body: {} }.
   *
   * @param context - The HTTP request context
   * @param settleResult - The settlement failure result
   * @returns An object containing both contentType and body for the 402 response
   */
  settlementFailedResponseBody?: SettlementFailedResponseBody;

  // Extensions
  extensions?: Record<string, unknown>;

  // ── period scheme additions (subscription-only) ──────────
  /**
   * When set, the route is a special-operation endpoint for the subscription
   * scheme:
   *   - "change": two-stage upgrade/downgrade flow
   *   - "cancel": cancel-subscription flow
   * Ignored by non-subscription schemes.
   */
  operation?: "change" | "cancel" | "cancel-pending-change";

  /**
   * Route-level hook fired AFTER `verifyAccess` succeeded (signature +
   * payer + plan allowlist + period math) but BEFORE the handler runs.
   * Seller uses it for custom access policy: rate limiting, quota, feature
   * flags, bans / blacklists (return `{ok:false, error:"banned"}`). Full
   * `Subscription` object is on the context so any field is inspectable.
   */
  onBeforeAccess?: import("../subscription").OnBeforeAccessHook;
}

/**
 * Routes configuration - maps path patterns to route configs
 */
export type RoutesConfig = Record<string, RouteConfig> | RouteConfig;

/**
 * Hook that runs on every request to a protected route, before payment processing.
 * Can grant access without payment, deny the request, or continue to payment flow.
 *
 * @returns
 * - `void` - Continue to payment processing (default behavior)
 * - `{ grantAccess: true }` - Grant access without requiring payment
 * - `{ abort: true; reason: string }` - Deny the request (returns 403)
 */
export type ProtectedRequestHook = (
  context: HTTPRequestContext,
  routeConfig: RouteConfig,
) => Promise<void | { grantAccess: true } | { abort: true; reason: string }>;

/**
 * Compiled route for efficient matching
 */
export interface CompiledRoute {
  verb: string;
  regex: RegExp;
  config: RouteConfig;
  pattern: string;
}

/**
 * HTTP request context that encapsulates all request data
 */
export interface HTTPRequestContext {
  adapter: HTTPAdapter;
  path: string;
  method: string;
  paymentHeader?: string;
  routePattern?: string;
}

/**
 * HTTP transport context contains both request context and optional response data.
 */
export interface HTTPTransportContext {
  /** The HTTP request context */
  request: HTTPRequestContext;
  /** The response body buffer */
  responseBody?: Buffer;
  /** Response headers set by the route handler (used for settlement overrides) */
  responseHeaders?: Record<string, string>;
}

/**
 * HTTP response instructions for the framework middleware
 */
export interface HTTPResponseInstructions {
  status: number;
  headers: Record<string, string>;
  body?: unknown; // e.g. Paywall for web browser requests, but could be any other type
  isHtml?: boolean; // e.g. if body is a paywall, then isHtml is true
}

/**
 * Result of processing an HTTP request for payment
 */
export type HTTPProcessResult =
  | { type: "no-payment-required" }
  | {
      type: "payment-verified";
      paymentPayload: PaymentPayload;
      paymentRequirements: PaymentRequirements;
      declaredExtensions?: Record<string, unknown>;
    }
  | { type: "payment-error"; response: HTTPResponseInstructions }
  // ── period dispatch results (produced by subscription-aware branches) ──
  | {
      type: "payment-presettle";
      paymentPayload: PaymentPayload;
      paymentRequirements: PaymentRequirements;
      /** Settle action to run AFTER verify; bound to scheme + operation. */
      settle: () => Promise<{
        success: boolean;
        headers?: Record<string, string>;
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        data?: any;
        error?: string;
      }>;
      operation: "subscribe" | "change" | "cancel" | "cancel-pending-change";
    }
  | {
      type: "access-verified";
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      subscription: import("../subscription").Subscription;
      headers?: Record<string, string>;
    };

/**
 * Result of processSettlement
 */
export type ProcessSettleSuccessResponse = SettleResponse & {
  success: true;
  headers: Record<string, string>;
  requirements: PaymentRequirements;
};

export type ProcessSettleFailureResponse = SettleResponse & {
  success: false;
  errorReason: string;
  errorMessage?: string;
  headers: Record<string, string>;
  response: HTTPResponseInstructions;
};

export type ProcessSettleResultResponse =
  | ProcessSettleSuccessResponse
  | ProcessSettleFailureResponse;

/**
 * Represents a validation error for a specific route's payment configuration.
 */
export interface RouteValidationError {
  /** The route pattern (e.g., "GET /api/weather") */
  routePattern: string;
  /** The payment scheme that failed validation */
  scheme: string;
  /** The network that failed validation */
  network: Network;
  /** The type of validation failure */
  reason: "missing_scheme" | "missing_facilitator";
  /** Human-readable error message */
  message: string;
}

/**
 * Error thrown when route configuration validation fails.
 */
export class RouteConfigurationError extends Error {
  /** The validation errors that caused this exception */
  public readonly errors: RouteValidationError[];

  /**
   * Creates a new RouteConfigurationError with the given validation errors.
   *
   * @param errors - The validation errors that caused this exception.
   */
  constructor(errors: RouteValidationError[]) {
    const message = `x402 Route Configuration Errors:\n${errors.map(e => `  - ${e.message}`).join("\n")}`;
    super(message);
    this.name = "RouteConfigurationError";
    this.errors = errors;
  }
}

/**
 * Hook called when the facilitator returns status="timeout".
 * Receives the tx hash and network so the server can verify on-chain.
 * Return { confirmed: true } to deliver the resource; { confirmed: false } to return 402.
 */
export type OnSettlementTimeoutHook = (
  txHash: string,
  network: string,
) => Promise<{ confirmed: boolean }>;

/**
 * HTTP-enhanced x402 resource server
 * Provides framework-agnostic HTTP protocol handling
 */
export class x402HTTPResourceServer {
  private ResourceServer: x402ResourceServer;
  private compiledRoutes: CompiledRoute[] = [];
  private routesConfig: RoutesConfig;
  private paywallProvider?: PaywallProvider;
  private protectedRequestHooks: ProtectedRequestHook[] = [];
  private timeoutRecoveryHook?: OnSettlementTimeoutHook;
  private beforeAccessHooks: import("../subscription").OnBeforeAccessHook[] = [];
  private pollDeadlineMs: number = DEFAULT_POLL_DEADLINE_MS;

  /**
   * Creates a new x402HTTPResourceServer instance.
   *
   * @param ResourceServer - The core x402ResourceServer instance to use
   * @param routes - Route configuration for payment-protected endpoints
   */
  constructor(ResourceServer: x402ResourceServer, routes: RoutesConfig) {
    this.ResourceServer = ResourceServer;
    this.routesConfig = routes;

    // Handle both single route and multiple routes
    const normalizedRoutes =
      typeof routes === "object" && !("accepts" in routes)
        ? (routes as Record<string, RouteConfig>)
        : { "*": routes as RouteConfig };

    for (const [pattern, config] of Object.entries(normalizedRoutes)) {
      const parsed = this.parseRoutePattern(pattern);
      this.compiledRoutes.push({
        verb: parsed.verb,
        regex: parsed.regex,
        config,
        pattern: parsed.path,
      });
    }
  }

  /**
   * Get the underlying x402ResourceServer instance.
   *
   * @returns The underlying x402ResourceServer instance
   */
  get server(): x402ResourceServer {
    return this.ResourceServer;
  }

  /**
   * Get the routes configuration.
   *
   * @returns The routes configuration
   */
  get routes(): RoutesConfig {
    return this.routesConfig;
  }

  /**
   * Initialize the HTTP resource server.
   *
   * This method initializes the underlying resource server (fetching facilitator support)
   * and then validates that all route payment configurations have corresponding
   * registered schemes and facilitator support.
   *
   * @throws RouteConfigurationError if any route's payment options don't have
   *         corresponding registered schemes or facilitator support
   *
   * @example
   * ```typescript
   * const httpServer = new x402HTTPResourceServer(server, routes);
   * await httpServer.initialize();
   * ```
   */
  async initialize(): Promise<void> {
    // First, initialize the underlying resource server (fetches facilitator support)
    await this.ResourceServer.initialize();

    // Then validate route configuration
    const errors = this.validateRouteConfiguration();
    if (errors.length > 0) {
      throw new RouteConfigurationError(errors);
    }
  }

  /**
   * Register a custom paywall provider for generating HTML
   *
   * @param provider - PaywallProvider instance
   * @returns This service instance for chaining
   */
  registerPaywallProvider(provider: PaywallProvider): this {
    this.paywallProvider = provider;
    return this;
  }

  /**
   * Register a hook that runs on every request to a protected route, before payment processing.
   * Hooks are executed in order of registration. The first hook to return a non-void result wins.
   *
   * @param hook - The request hook function
   * @returns The x402HTTPResourceServer instance for chaining
   */
  onProtectedRequest(hook: ProtectedRequestHook): this {
    this.protectedRequestHooks.push(hook);
    return this;
  }

  /**
   * Register a seller-global `onBeforeAccess` hook fired on every access-
   * verified subscription request, AFTER `verifyAccess` (signature + payer
   * + plan allowlist + period math) but BEFORE the handler runs. Seller
   * uses it for cross-cutting access policy (quota / ban list / feature
   * gating). Hooks are executed in order of registration; the first one
   * to return `{ ok: false }` denies (→ 402). Route-level
   * `RouteConfig.onBeforeAccess` runs AFTER all global hooks.
   *
   * @param hook - The hook function
   * @returns The x402HTTPResourceServer instance for chaining
   */
  onBeforeAccess(hook: import("../subscription").OnBeforeAccessHook): this {
    this.beforeAccessHooks.push(hook);
    return this;
  }

  /**
   * Register a hook to call when the facilitator returns status="timeout".
   * The hook should verify the tx on-chain and return { confirmed: boolean }.
   * If confirmed=true the resource is delivered (200); otherwise 402 is returned.
   *
   * @param hook - On-chain verification callback
   * @returns The x402HTTPResourceServer instance for chaining
   */
  onSettlementTimeout(hook: OnSettlementTimeoutHook): this {
    this.timeoutRecoveryHook = hook;
    return this;
  }

  /**
   * Set the poll deadline for settle/status polling on timeout recovery.
   * Default is 5000ms.
   *
   * @param deadlineMs - Maximum time to poll in milliseconds
   * @returns The x402HTTPResourceServer instance for chaining
   */
  setPollDeadline(deadlineMs: number): this {
    this.pollDeadlineMs = deadlineMs;
    return this;
  }

  /**
   * Process HTTP request and return response instructions
   * This is the main entry point for framework middleware
   *
   * @param context - HTTP request context
   * @param paywallConfig - Optional paywall configuration
   * @returns Process result indicating next action for middleware
   */
  async processHTTPRequest(
    context: HTTPRequestContext,
    paywallConfig?: PaywallConfig,
  ): Promise<HTTPProcessResult> {
    const { adapter, path, method } = context;

    // Find matching route
    const routeMatch = this.getRouteConfig(path, method);
    if (!routeMatch) {
      return { type: "no-payment-required" }; // No payment required for this route
    }
    const { config: routeConfig, pattern: routePattern } = routeMatch;
    const enrichedContext: HTTPRequestContext = { ...context, routePattern };

    // Execute request hooks before any payment processing
    for (const hook of this.protectedRequestHooks) {
      const result = await hook(enrichedContext, routeConfig);
      if (result && "grantAccess" in result) {
        return { type: "no-payment-required" };
      }
      if (result && "abort" in result) {
        return {
          type: "payment-error",
          response: {
            status: 403,
            headers: { "Content-Type": "application/json" },
            body: { error: result.reason },
          },
        };
      }
    }

    // Normalize accepts field to array of payment options
    const paymentOptions = this.normalizePaymentOptions(routeConfig);

    // Check for payment header (v1 or v2) — used by Branches A / B / D.
    const paymentPayload = this.extractPayment(adapter);

    // ── period Branch A: change endpoint ──────────────────
    // If the route is declared with operation:"change", drive the two-stage
    // Change route: no special dispatcher. Buyer follows the standard
    // x402 flow — GET the route → 402 with accepts → pick one → sign newTerms
    // with `changeFromSubId` set to the current sub → POST + APP-PAYMENT.
    // The presettle branch below routes to verifyChange / settleChange based
    // on routeConfig.operation.

    // ── period Branch B: cancel endpoint ──────────────────────────────────
    if (routeConfig.operation === "cancel") {
      const cancelResult = await this.tryDispatchCancelFlow(adapter, routeConfig, paymentOptions);
      if (cancelResult) return cancelResult;
    }

    // ── period Branch B2: cancel-pending-change endpoint ────────────────
    if (routeConfig.operation === "cancel-pending-change") {
      const r = await this.tryDispatchCancelPendingChangeFlow(adapter, routeConfig, paymentOptions);
      if (r) return r;
    }

    // ── period Branch C: APP-Access (access flow) ────────
    // Build PaymentRequired once. Subscribe presettle + classic verify path
    // both consume the same `accepts[]` — building it here (instead of inside
    // tryDispatchSubscriptionPresettle + again below) eliminates the duplicate
    // facilitator-supportedKinds lookup and parsePrice/enhance pipeline.
    //
    // access / change / cancel flows don't depend on this; they've already
    // short-circuited above. The cost of building requirements once on the
    // way to subscribe / classic dispatch is negligible.
    const resourceInfo = {
      url: routeConfig.resource || enrichedContext.adapter.getUrl(),
      description: routeConfig.description || "",
      mimeType: routeConfig.mimeType || "",
    };
    let requirements = await this.ResourceServer.buildPaymentRequirementsFromOptions(
      paymentOptions,
      enrichedContext,
    );

    // Change route: inject `extra.changeFrom` into each accept so the 402
    // tells the buyer exactly which sub they're switching from and which
    // direction (upgrade/downgrade). Same-tier accepts are filtered out
    // (a no-op change is illegal — `tier_same`).
    //
    // currentSubId source per phase:
    //   - sniff phase (no PAYMENT-SIGNATURE) → APP-Access header carrying
    //     a buyer-signed AccessProof; middleware ecrecovers it and reads
    //     proof.subId. Prevents subId-only reconnaissance attacks.
    //   - settle phase (PAYMENT-SIGNATURE)   → `terms.changeFromSubId` from
    //     the buyer-signed newTerms (tamper-proof, signature covers it).
    //
    // Both phases MUST produce the same enriched accepts so verifyChange's
    // `findMatchingRequirements` deepEqual succeeds.
    if (routeConfig.operation === "change") {
      // Resolve the subscription scheme first — both phases need it.
      let scheme: SubscriptionCapability | null = null;
      for (const opt of paymentOptions) {
        if (!opt.network || !opt.scheme) continue;
        scheme = await this.resolveSubscriptionScheme(opt.network, opt.scheme);
        if (scheme) break;
      }
      if (!scheme) {
        return {
          type: "payment-error",
          response: {
            status: 500,
            headers: { "Content-Type": "application/json" },
            body: { error: "change route: no subscription scheme registered" },
          },
        };
      }

      let currentSubId: string | undefined;
      if (paymentPayload) {
        const innerTerms = (
          paymentPayload.payload as { terms?: { changeFromSubId?: string } } | undefined
        )?.terms;
        currentSubId = innerTerms?.changeFromSubId;
      } else {
        const accessHeader = this.extractAccessProofHeader(enrichedContext.adapter);
        if (!accessHeader) {
          return {
            type: "payment-error",
            response: {
              status: 401,
              headers: { "Content-Type": "application/json" },
              body: { error: "change route: missing APP-Access header" },
            },
          };
        }
        const { decodeAccessProof } = await this.loadSubscriptionModule();
        let proof;
        try {
          proof = decodeAccessProof(accessHeader);
        } catch {
          return {
            type: "payment-error",
            response: {
              status: 400,
              headers: { "Content-Type": "application/json" },
              body: { error: "change route: invalid APP-Access header" },
            },
          };
        }
        // change-route sniff: prove ownership only. `verifyAccess`'s
        // plan-allowlist + period-math gating is meant for resource
        // consumption — it would (wrongly) reject a buyer whose current
        // period hasn't been charged yet from even seeing change offers.
        // `verifyOwnership` does just sig + store + payer match.
        const verify = await scheme.verifyOwnership(proof);
        if (!verify.ok) {
          return {
            type: "payment-error",
            response: {
              status: 401,
              headers: { "Content-Type": "application/json" },
              body: { error: verify.error },
            },
          };
        }
        currentSubId = verify.subId;
      }
      if (!currentSubId) {
        return {
          type: "payment-error",
          response: {
            status: 400,
            headers: { "Content-Type": "application/json" },
            body: { error: "change route: cannot resolve currentSubId" },
          },
        };
      }
      const enriched = await scheme.enrichAcceptsForChange(requirements, currentSubId);
      if (enriched === null) {
        return {
          type: "payment-error",
          response: {
            status: 404,
            headers: { "Content-Type": "application/json" },
            body: { error: "sub_not_active_for_change" },
          },
        };
      }
      requirements = enriched;
    }

    let extensions = routeConfig.extensions;
    if (extensions) {
      extensions = this.ResourceServer.enrichExtensions(extensions, enrichedContext);
    }
    const transportContext: HTTPTransportContext = { request: enrichedContext };
    const paymentRequired = await this.ResourceServer.createPaymentRequiredResponse(
      requirements,
      resourceInfo,
      !paymentPayload ? "Payment required" : undefined,
      extensions,
      transportContext,
    );

    // Access flow runs AFTER paymentRequired so that an AccessProof rejection
    // (sub not in this route's plan allowlist / expired / etc.) can return a
    // proper 402 + PAYMENT-REQUIRED body with the route's accepts — buyer
    // sees which plans they'd need to subscribe to gain access.
    //
    // Skip for change routes: APP-Access on a change route is the buyer's
    // identity for `extra.changeFrom` enrichment (above), NOT a request to
    // access the resource itself.
    if (routeConfig.operation !== "change") {
      const accessResult = await this.tryDispatchAccessFlow(
        adapter,
        routeConfig,
        paymentOptions,
        paymentRequired,
      );
      if (accessResult) return accessResult;
    }

    // ── period Branch D: APP-PAYMENT + scheme="pre" ──────
    // If the buyer presented a subscription PaymentPayload (scheme tags itself
    // as `period` and the registered server declares
    // `settlementMode === "pre"`), run verify+settle BEFORE handler executes.
    if (paymentPayload) {
      const subResult = await this.tryDispatchSubscriptionPresettle(
        paymentPayload,
        paymentRequired.accepts,
        routeConfig.operation === "change" ? "change" : "subscribe",
      );
      if (subResult) return subResult;
    }

    // If no payment provided
    if (!paymentPayload) {
      // Resolve custom unpaid response body if provided
      const unpaidBody = routeConfig.unpaidResponseBody
        ? await routeConfig.unpaidResponseBody(enrichedContext)
        : undefined;

      return {
        type: "payment-error",
        response: this.createHTTPResponse(
          paymentRequired,
          this.isWebBrowser(adapter),
          paywallConfig,
          routeConfig.customPaywallHtml,
          unpaidBody,
        ),
      };
    }

    // Verify payment
    try {
      const matchingRequirements = this.ResourceServer.findMatchingRequirements(
        paymentRequired.accepts,
        paymentPayload,
      );

      if (!matchingRequirements) {
        const errorResponse = await this.ResourceServer.createPaymentRequiredResponse(
          requirements,
          resourceInfo,
          "No matching payment requirements",
          routeConfig.extensions,
          transportContext,
        );
        return {
          type: "payment-error",
          response: this.createHTTPResponse(errorResponse, false, paywallConfig),
        };
      }

      const verifyResult = await this.ResourceServer.verifyPayment(
        paymentPayload,
        matchingRequirements,
      );

      if (!verifyResult.isValid) {
        const errorResponse = await this.ResourceServer.createPaymentRequiredResponse(
          requirements,
          resourceInfo,
          verifyResult.invalidReason,
          routeConfig.extensions,
          transportContext,
        );
        return {
          type: "payment-error",
          response: this.createHTTPResponse(errorResponse, false, paywallConfig),
        };
      }

      // Payment is valid, return data needed for settlement
      return {
        type: "payment-verified",
        paymentPayload,
        paymentRequirements: matchingRequirements,
        declaredExtensions: routeConfig.extensions,
      };
    } catch (error) {
      if (error instanceof FacilitatorResponseError) {
        throw error;
      }
      const errorResponse = await this.ResourceServer.createPaymentRequiredResponse(
        requirements,
        resourceInfo,
        error instanceof Error ? error.message : "Payment verification failed",
        routeConfig.extensions,
        transportContext,
      );
      return {
        type: "payment-error",
        response: this.createHTTPResponse(errorResponse, false, paywallConfig),
      };
    }
  }

  /**
   * Process settlement after successful response
   *
   * @param paymentPayload - The verified payment payload
   * @param requirements - The matching payment requirements
   * @param declaredExtensions - Optional declared extensions (for per-key enrichment)
   * @param transportContext - Optional HTTP transport context
   * @param settlementOverrides - Optional settlement overrides (e.g., partial settlement amount)
   * @returns ProcessSettleResultResponse - SettleResponse with headers if success or errorReason if failure
   */
  async processSettlement(
    paymentPayload: PaymentPayload,
    requirements: PaymentRequirements,
    declaredExtensions?: Record<string, unknown>,
    transportContext?: HTTPTransportContext,
    settlementOverrides?: SettlementOverrides,
  ): Promise<ProcessSettleResultResponse> {
    try {
      // Resolve overrides: explicit param takes precedence, fall back to response header
      let resolvedOverrides = settlementOverrides;
      if (!resolvedOverrides && transportContext?.responseHeaders?.[SETTLEMENT_OVERRIDES_HEADER]) {
        try {
          resolvedOverrides = JSON.parse(
            transportContext.responseHeaders[SETTLEMENT_OVERRIDES_HEADER],
          );
        } catch {
          // Ignore malformed header
        }
      }

      const settleResponse = await this.ResourceServer.settlePayment(
        paymentPayload,
        requirements,
        declaredExtensions,
        transportContext,
        resolvedOverrides,
      );

      // status="timeout" → facilitator gave up waiting for on-chain confirmation.
      // Step 1: Poll /settle/status (interval 1s, deadline from config).
      // Step 2: If polling fails/times out, try developer's timeout hook.
      if (settleResponse.status === "timeout") {
        if (settleResponse.transaction) {
          // Step 1: Poll settle/status
          const pollResult = await this.ResourceServer.pollSettleStatus(
            settleResponse.transaction,
            paymentPayload,
            requirements,
            this.pollDeadlineMs,
          );

          if (pollResult === "success") {
            // Poll confirmed on-chain → deliver resource
            const recovered = { ...settleResponse, status: "success" as const };
            return {
              ...recovered,
              success: true as const,
              headers: this.createSettlementHeaders(recovered),
              requirements,
            };
          }

          // Step 2: Poll unsuccessful → try developer's timeout hook
          if (this.timeoutRecoveryHook) {
            try {
              const { confirmed } = await this.timeoutRecoveryHook(
                settleResponse.transaction,
                settleResponse.network as string,
              );
              if (confirmed) {
                const recovered = { ...settleResponse, status: "success" as const };
                return {
                  ...recovered,
                  success: true as const,
                  headers: this.createSettlementHeaders(recovered),
                  requirements,
                };
              }
            } catch (err) {
              console.warn("[x402] onSettlementTimeout hook error:", err);
            }
          }
        }

        // No transaction, or poll+hook both failed → 402
        const failure = {
          ...settleResponse,
          success: false as const,
          errorReason: "settlement_timeout",
          errorMessage: "Settlement timed out waiting for on-chain confirmation",
          headers: this.createSettlementHeaders(settleResponse),
        };
        const response = await this.buildSettlementFailureResponse(failure, transportContext);
        return { ...failure, response };
      }

      // status="success" → on-chain confirmed (syncSettle=true), deliver resource
      // status="pending" → async mode, Seller trusts Facilitator, deliver resource
      // no status → standard okx facilitator, check success field
      if (settleResponse.status === "success" || settleResponse.status === "pending") {
        return {
          ...settleResponse,
          success: true as const,
          headers: this.createSettlementHeaders(settleResponse),
          requirements,
        };
      }

      // Fallback: standard okx facilitator (no status field)
      if (!settleResponse.success) {
        const failure = {
          ...settleResponse,
          success: false as const,
          errorReason: settleResponse.errorReason || "Settlement failed",
          errorMessage:
            settleResponse.errorMessage || settleResponse.errorReason || "Settlement failed",
          headers: this.createSettlementHeaders(settleResponse),
        };
        const response = await this.buildSettlementFailureResponse(failure, transportContext);
        return { ...failure, response };
      }

      return {
        ...settleResponse,
        success: true,
        headers: this.createSettlementHeaders(settleResponse),
        requirements,
      };
    } catch (error) {
      if (error instanceof FacilitatorResponseError) {
        throw error;
      }
      if (error instanceof SettleError) {
        const errorReason = error.errorReason || error.message;
        const settleResponse: SettleResponse = {
          success: false,
          errorReason,
          errorMessage: error.errorMessage || errorReason,
          payer: error.payer,
          network: error.network,
          transaction: error.transaction,
        };
        const failure = {
          ...settleResponse,
          success: false as const,
          errorReason,
          headers: this.createSettlementHeaders(settleResponse),
        };
        const response = await this.buildSettlementFailureResponse(failure, transportContext);
        return { ...failure, response };
      }
      const errorReason = error instanceof Error ? error.message : "Settlement failed";
      const settleResponse: SettleResponse = {
        success: false,
        errorReason,
        errorMessage: errorReason,
        network: requirements.network as Network,
        transaction: "",
      };
      const failure = {
        ...settleResponse,
        success: false as const,
        errorReason,
        headers: this.createSettlementHeaders(settleResponse),
      };
      const response = await this.buildSettlementFailureResponse(failure, transportContext);
      return { ...failure, response };
    }
  }

  /**
   * Check if a request requires payment based on route configuration
   *
   * @param context - HTTP request context
   * @returns True if the route requires payment, false otherwise
   */
  requiresPayment(context: HTTPRequestContext): boolean {
    return this.getRouteConfig(context.path, context.method) !== undefined;
  }

  /**
   * Lazy loader for the subscription submodule. The `import()` cache makes
   * this effectively free after the first hit; isolating it in one place
   * keeps dispatch helpers free of dynamic-import boilerplate and lets
   * bundlers tree-shake the entire subscription path when no caller touches
   * it.
   */
  protected loadSubscriptionModule(): Promise<typeof import("../subscription")> {
    return import("../subscription");
  }

  /**
   * Single chokepoint for "is this (network, scheme) backed by a
   * SubscriptionCapability-implementing scheme?". Returns the narrowed
   * capability (so callers get full typing on `verifyAccess` / `verifySubscribe`
   * / etc.) or null if not registered or not a subscription scheme.
   */
  protected async resolveSubscriptionScheme(
    network: Network,
    schemeName: string,
  ): Promise<SubscriptionCapability | null> {
    const registered = this.ResourceServer.findScheme(network, schemeName);
    if (!registered) return null;
    const { hasSubscriptionCapability } = await this.loadSubscriptionModule();
    return hasSubscriptionCapability(registered) ? registered : null;
  }

  /**
   * period dispatch helper — Access flow.
   *
   * Returns an `access-verified` (or `payment-error`) HTTPProcessResult when
   * the request carries `APP-Access` AND a subscription-capable scheme is
   * registered for one of the route's accepted (scheme, network) pairs.
   * Returns `null` to indicate the dispatcher should fall through to classic
   * pay-per-request handling.
   */
  protected async tryDispatchAccessFlow(
    adapter: HTTPAdapter,
    routeConfig: RouteConfig,
    paymentOptions: PaymentOption[],
    paymentRequired: PaymentRequired,
  ): Promise<HTTPProcessResult | null> {
    const headerB64 = this.extractAccessProofHeader(adapter);
    if (!headerB64) return null;

    const { decodeAccessProof } = await this.loadSubscriptionModule();

    let proof;
    try {
      proof = decodeAccessProof(headerB64);
    } catch (err) {
      return {
        type: "payment-error",
        response: {
          status: 401,
          headers: { "Content-Type": "application/json" },
          body: { error: `invalid APP-Access: ${(err as Error).message}` },
        },
      };
    }

    // Derive the route's allowed plan-id allowlist from its `accepts` —
    // every accept entry's `extra.plan.id` is one acceptable plan. A
    // subscription's `planId` must appear here to satisfy this route.
    const acceptedPlanIds = collectAcceptedPlanIds(paymentOptions);

    // First (scheme, network) in the route's accepts that resolves to a
    // SubscriptionCapability wins. v1 assumes one subscription scheme per
    // route.
    for (const opt of paymentOptions) {
      if (!opt.network || !opt.scheme) continue;
      const scheme = await this.resolveSubscriptionScheme(opt.network, opt.scheme);
      if (!scheme) continue;
      const result = await scheme.verifyAccess(proof, { acceptedPlanIds });
      if (!result.ok) {
        // 402: PAYMENT-REQUIRED header carries the route's accepts (so buyer
        // can subscribe / change to a satisfying plan); body carries the
        // specific reason code (subscription_not_active / signature_invalid
        // / payer_mismatch / …).
        return {
          type: "payment-error",
          response: {
            status: 402,
            headers: {
              "Content-Type": "application/json",
              "PAYMENT-REQUIRED": encodePaymentRequiredHeader(paymentRequired),
            },
            body: { error: result.error },
          },
        };
      }

      // `onBeforeAccess` hooks: seller-defined access policy
      // (quota, ban list, feature gating…). Full Subscription is passed so
      // arbitrary fields (subId / payer / planId / lastChargedPeriod / …)
      // can drive the decision. Return `{ok:false}` to deny. Global hook
      // (set via `httpServer.onBeforeAccess()`) runs FIRST; if it passes,
      // route-level hook (in `RouteConfig.onBeforeAccess`) runs after.
      const hooks: import("../subscription").OnBeforeAccessHook[] = [
        ...this.beforeAccessHooks,
        ...(routeConfig.onBeforeAccess ? [routeConfig.onBeforeAccess] : []),
      ];
      for (const hook of hooks) {
        const decision = await hook({
          subscription: result.subscription,
          request: {
            path: adapter.getPath(),
            method: adapter.getMethod(),
            headers: adapter.getHeaders?.() ?? {},
          },
          route: { acceptedPlanIds, accepts: paymentRequired.accepts },
        });
        if (!decision.ok) {
          return {
            type: "payment-error",
            response: {
              status: 402,
              headers: { "Content-Type": "application/json" },
              body: {
                error: decision.error ?? "access_denied",
                retryAfter: decision.retryAfter,
                upgradeOffers: decision.upgradeOffers,
              },
            },
          };
        }
      }

      return {
        type: "access-verified",
        subscription: result.subscription,
        headers: {},
      };
    }

    // No subscription-capable scheme registered for any of the route's
    // accepted networks. Treat as unauthorized rather than fall through to a
    // 402 paywall, since the buyer clearly tried the access path.
    return {
      type: "payment-error",
      response: {
        status: 401,
        headers: { "Content-Type": "application/json" },
        body: { error: "no subscription scheme registered for this route" },
      },
    };
  }

  /**
   * period dispatch helper — Subscribe presettle flow.
   *
   * When the buyer presents a PaymentPayload whose `accepted.scheme` is a
   * subscription scheme with `settlementMode === "pre"`, this runs verify +
   * (settle on demand) and returns `payment-presettle`. The middleware is
   * expected to call `result.settle()` AFTER decision-time but BEFORE
   * `next()` so handler only runs when the chain creation succeeded.
   *
   * Returns `null` to fall through to classic post-settle path-verified flow.
   */
  protected async tryDispatchSubscriptionPresettle(
    paymentPayload: PaymentPayload,
    serverAccepts: PaymentRequirements[],
    operation: "subscribe" | "change",
  ): Promise<HTTPProcessResult | null> {
    const { accepted } = paymentPayload;
    const scheme = await this.resolveSubscriptionScheme(accepted.network, accepted.scheme);
    if (!scheme) return null;

    // ★ Security gate (anti-tampering): caller passes already-built
    // `serverAccepts` (the seller's authoritative requirements from route
    // config). We look up which entry the buyer's PaymentPayload matches —
    // returning the SERVER copy, not the buyer's. Divergence on base fields
    // or server-declared extra → 402 here, well before any facilitator call.
    const serverReq = this.ResourceServer.findMatchingRequirements(serverAccepts, paymentPayload);
    if (!serverReq) {
      return {
        type: "payment-error",
        response: {
          status: 402,
          headers: { "Content-Type": "application/json" },
          body: { error: "no_matching_requirements" },
        },
      };
    }

    if (operation === "change") {
      const verifyResult = await scheme.verifyChange(paymentPayload, serverReq);
      if (!verifyResult.ok) {
        return {
          type: "payment-error",
          response: {
            status: 402,
            headers: { "Content-Type": "application/json" },
            body: { error: verifyResult.error },
          },
        };
      }
      return {
        type: "payment-presettle",
        paymentPayload,
        paymentRequirements: serverReq,
        operation: "change",
        settle: async () => {
          const r = await scheme.settleChange(paymentPayload, serverReq);
          return r.success
            ? {
                success: true,
                headers: r.headers,
                data: {
                  newSubId: r.newSubId,
                  oldSubId: r.oldSubId,
                  operationType: r.operationType,
                  scheduledFromPeriod: r.scheduledFromPeriod,
                },
              }
            : { success: false, error: r.error };
        },
      };
    }

    const verifyResult = await scheme.verifySubscribe(paymentPayload, serverReq);
    if (!verifyResult.ok) {
      return {
        type: "payment-error",
        response: {
          status: 402,
          headers: { "Content-Type": "application/json" },
          body: { error: verifyResult.error },
        },
      };
    }

    return {
      type: "payment-presettle",
      paymentPayload,
      paymentRequirements: serverReq,
      operation: "subscribe",
      settle: async () => {
        const r = await scheme.settleSubscribe(paymentPayload, serverReq);
        return r.success
          ? {
              success: true,
              headers: r.headers,
              data: { subId: r.subId, subscription: r.subscription },
            }
          : { success: false, error: r.error };
      },
    };
  }

  /**
   * period dispatch helper — Cancel flow.
   *
   * Reads JSON body { auth: CancelAuth, subId: string }, runs verifyCancel
   * then wraps settleCancel as a payment-presettle (settle-before-handler so
   * the cancelation is on-chain before the seller's response).
   */
  protected async tryDispatchCancelFlow(
    adapter: HTTPAdapter,
    routeConfig: RouteConfig,
    paymentOptions: PaymentOption[],
  ): Promise<HTTPProcessResult | null> {
    let scheme: SubscriptionCapability | null = null;
    for (const opt of paymentOptions) {
      if (!opt.network || !opt.scheme) continue;
      const resolved = await this.resolveSubscriptionScheme(opt.network as Network, opt.scheme);
      if (resolved) {
        scheme = resolved;
        break;
      }
    }
    if (!scheme) return null;

    const body = (adapter.getBody?.() ?? {}) as {
      auth?: import("../subscription").CancelAuth;
      subId?: string;
    };
    if (!body.auth || !body.subId) {
      return {
        type: "payment-error",
        response: {
          status: 400,
          headers: { "Content-Type": "application/json" },
          body: { error: "cancel: body must include auth and subId" },
        },
      };
    }

    const verifyResult = await scheme.verifyCancel(body.auth, body.subId);
    if (!verifyResult.ok) {
      return {
        type: "payment-error",
        response: {
          status: 402,
          headers: { "Content-Type": "application/json" },
          body: { error: verifyResult.error },
        },
      };
    }

    void routeConfig;
    const settleScheme = scheme;
    const auth = body.auth;
    const subId = body.subId;
    return {
      type: "payment-presettle",
      paymentPayload: { x402Version: 2, accepted: null as never, payload: {} },
      paymentRequirements: null as never,
      operation: "cancel",
      settle: async () => {
        const r = await settleScheme.settleCancel(auth, subId);
        return r.success
          ? { success: true, headers: r.headers, data: { subId } }
          : { success: false, error: r.error };
      },
    };
  }

  /**
   * period dispatch helper — Cancel-Pending-Change flow.
   *
   * Reads JSON body `{ auth: PendingChangeCancelAuth, subId: string }`. The
   * auth must carry `newSubId` (matches the currently PENDING downgrade
   * target). Runs verifyCancelPendingChange then wraps
   * settleCancelPendingChange as a payment-presettle.
   */
  protected async tryDispatchCancelPendingChangeFlow(
    adapter: HTTPAdapter,
    routeConfig: RouteConfig,
    paymentOptions: PaymentOption[],
  ): Promise<HTTPProcessResult | null> {
    let scheme: SubscriptionCapability | null = null;
    for (const opt of paymentOptions) {
      if (!opt.network || !opt.scheme) continue;
      const resolved = await this.resolveSubscriptionScheme(opt.network as Network, opt.scheme);
      if (resolved) {
        scheme = resolved;
        break;
      }
    }
    if (!scheme) return null;

    const body = (adapter.getBody?.() ?? {}) as {
      auth?: import("../subscription").PendingChangeCancelAuth;
      subId?: string;
    };
    if (!body.auth || !body.subId) {
      return {
        type: "payment-error",
        response: {
          status: 400,
          headers: { "Content-Type": "application/json" },
          body: { error: "cancel-pending-change: body must include auth and subId" },
        },
      };
    }
    if (!body.auth.newSubId) {
      // Facilitator requires newSubId — reject early with a clear code.
      return {
        type: "payment-error",
        response: {
          status: 400,
          headers: { "Content-Type": "application/json" },
          body: { error: "cancel-pending-change: auth.newSubId is required" },
        },
      };
    }

    const verifyResult = await scheme.verifyCancelPendingChange(body.auth, body.subId);
    if (!verifyResult.ok) {
      return {
        type: "payment-error",
        response: {
          status: 402,
          headers: { "Content-Type": "application/json" },
          body: { error: verifyResult.error },
        },
      };
    }

    void routeConfig;
    const settleScheme = scheme;
    const auth = body.auth;
    const subId = body.subId;
    return {
      type: "payment-presettle",
      paymentPayload: { x402Version: 2, accepted: null as never, payload: {} },
      paymentRequirements: null as never,
      operation: "cancel-pending-change",
      settle: async () => {
        const r = await settleScheme.settleCancelPendingChange(auth, subId);
        return r.success
          ? { success: true, headers: r.headers, data: { subId: r.subId } }
          : { success: false, error: r.error };
      },
    };
  }

  /**
   * Build HTTPResponseInstructions for settlement failure.
   * Uses settlementFailedResponseBody hook if configured, otherwise defaults to empty body.
   *
   * @param failure - Settlement failure result with headers
   * @param transportContext - Optional HTTP transport context for the request
   * @returns HTTP response instructions for the 402 settlement failure response
   */
  private async buildSettlementFailureResponse(
    failure: Omit<ProcessSettleFailureResponse, "response">,
    transportContext?: HTTPTransportContext,
  ): Promise<HTTPResponseInstructions> {
    const settlementHeaders = failure.headers;
    const routeConfig = transportContext
      ? this.getRouteConfig(transportContext.request.path, transportContext.request.method)
      : undefined;

    const customBody = routeConfig?.config.settlementFailedResponseBody
      ? await routeConfig.config.settlementFailedResponseBody(transportContext!.request, failure)
      : undefined;

    const contentType = customBody ? customBody.contentType : "application/json";
    const body = customBody ? customBody.body : {};

    return {
      status: 402,
      headers: {
        "Content-Type": contentType,
        ...settlementHeaders,
      },
      body,
      isHtml: contentType.includes("text/html"),
    };
  }

  /**
   * Normalizes a RouteConfig's accepts field into an array of PaymentOptions
   * Handles both single PaymentOption and array formats
   *
   * @param routeConfig - Route configuration
   * @returns Array of payment options
   */
  private normalizePaymentOptions(routeConfig: RouteConfig): PaymentOption[] {
    return Array.isArray(routeConfig.accepts) ? routeConfig.accepts : [routeConfig.accepts];
  }

  /**
   * Validates that all payment options in routes have corresponding registered schemes
   * and facilitator support.
   *
   * @returns Array of validation errors (empty if all routes are valid)
   */
  private validateRouteConfiguration(): RouteValidationError[] {
    const errors: RouteValidationError[] = [];

    // Normalize routes to array of [pattern, config] pairs
    const normalizedRoutes =
      typeof this.routesConfig === "object" && !("accepts" in this.routesConfig)
        ? Object.entries(this.routesConfig as Record<string, RouteConfig>)
        : [["*", this.routesConfig as RouteConfig] as [string, RouteConfig]];

    for (const [pattern, config] of normalizedRoutes) {
      // Warn if wildcard routes are used with discovery extensions
      const pathPart = pattern.includes(" ") ? pattern.split(/\s+/)[1] : pattern;
      if (
        pathPart &&
        pathPart.includes("*") &&
        config.extensions &&
        "bazaar" in config.extensions
      ) {
        console.warn(
          `[x402] Route "${pattern}": Wildcard (*) patterns with bazaar discovery extensions ` +
            `will auto-generate parameter names (var1, var2, ...). ` +
            `Consider using named parameters instead (e.g. /weather/:city) for better discovery metadata.`,
        );
      }

      const paymentOptions = this.normalizePaymentOptions(config);

      for (const option of paymentOptions) {
        // Check 1: Is scheme registered?
        if (!this.ResourceServer.hasRegisteredScheme(option.network, option.scheme)) {
          errors.push({
            routePattern: pattern,
            scheme: option.scheme,
            network: option.network,
            reason: "missing_scheme",
            message: `Route "${pattern}": No scheme implementation registered for "${option.scheme}" on network "${option.network}"`,
          });
          // Skip facilitator check if scheme isn't registered
          continue;
        }

        // Check 2: Does facilitator support this scheme/network combination?
        const supportedKind = this.ResourceServer.getSupportedKind(
          x402Version,
          option.network,
          option.scheme,
        );

        if (!supportedKind) {
          errors.push({
            routePattern: pattern,
            scheme: option.scheme,
            network: option.network,
            reason: "missing_facilitator",
            message: `Route "${pattern}": Facilitator does not support scheme "${option.scheme}" on network "${option.network}"`,
          });
        }
      }
    }

    return errors;
  }

  /**
   * Get route configuration for a request
   *
   * @param path - Request path
   * @param method - HTTP method
   * @returns Route configuration and pattern, or undefined if no match
   */
  private getRouteConfig(
    path: string,
    method: string,
  ): { config: RouteConfig; pattern: string } | undefined {
    const normalizedPath = this.normalizePath(path);
    const upperMethod = method.toUpperCase();

    const matchingRoute = this.compiledRoutes.find(
      route =>
        route.regex.test(normalizedPath) && (route.verb === "*" || route.verb === upperMethod),
    );

    if (!matchingRoute) return undefined;
    return { config: matchingRoute.config, pattern: matchingRoute.pattern };
  }

  /**
   * Extract payment from HTTP headers (handles v1 and v2)
   *
   * @param adapter - HTTP adapter
   * @returns Decoded payment payload or null
   */
  private extractPayment(adapter: HTTPAdapter): PaymentPayload | null {
    // Check v2 header first (PAYMENT-SIGNATURE)
    const header = adapter.getHeader("payment-signature") || adapter.getHeader("PAYMENT-SIGNATURE");

    if (header) {
      try {
        return decodePaymentSignatureHeader(header);
      } catch (error) {
        console.warn("Failed to decode PAYMENT-SIGNATURE header:", error);
      }
    }

    // period uses APP-PAYMENT (base64-encoded JSON).
    const subHeader = adapter.getHeader("app-payment") || adapter.getHeader("APP-PAYMENT");
    if (subHeader) {
      try {
        const json = Buffer.from(subHeader, "base64").toString("utf8");
        return JSON.parse(json) as PaymentPayload;
      } catch (error) {
        console.warn("Failed to decode APP-PAYMENT header:", error);
      }
    }

    return null;
  }

  /**
   * Extract `APP-Access` header (subscription access-flow). Returns the raw
   * base64 string so callers can pass it through to `decodeAccessProof` in
   * the subscription codec.
   */
  private extractAccessProofHeader(adapter: HTTPAdapter): string | null {
    return adapter.getHeader("app-access") || adapter.getHeader("APP-Access") || null;
  }

  /**
   * Check if request is from a web browser
   *
   * @param adapter - HTTP adapter
   * @returns True if request appears to be from a browser
   */
  private isWebBrowser(adapter: HTTPAdapter): boolean {
    const accept = adapter.getAcceptHeader();
    const userAgent = adapter.getUserAgent();
    return accept.includes("text/html") && userAgent.includes("Mozilla");
  }

  /**
   * Create HTTP response instructions from payment required
   *
   * @param paymentRequired - Payment requirements
   * @param isWebBrowser - Whether request is from browser
   * @param paywallConfig - Paywall configuration
   * @param customHtml - Custom HTML template
   * @param unpaidResponse - Optional custom response (content type and body) for unpaid API requests
   * @returns Response instructions
   */
  private createHTTPResponse(
    paymentRequired: PaymentRequired,
    isWebBrowser: boolean,
    paywallConfig?: PaywallConfig,
    customHtml?: string,
    unpaidResponse?: HTTPResponseBody,
  ): HTTPResponseInstructions {
    // Use 412 Precondition Failed for permit2_allowance_required error
    // This signals client needs to approve Permit2 before retrying
    const status = paymentRequired.error === "permit2_allowance_required" ? 412 : 402;

    if (isWebBrowser) {
      const html = this.generatePaywallHTML(paymentRequired, paywallConfig, customHtml);
      return {
        status,
        headers: { "Content-Type": "text/html" },
        body: html,
        isHtml: true,
      };
    }

    const response = this.createHTTPPaymentRequiredResponse(paymentRequired);

    // Use callback result if provided, otherwise default to JSON with empty object
    const contentType = unpaidResponse ? unpaidResponse.contentType : "application/json";
    const body = unpaidResponse ? unpaidResponse.body : {};

    return {
      status,
      headers: {
        "Content-Type": contentType,
        ...response.headers,
      },
      body,
    };
  }

  /**
   * Create HTTP payment required response (v1 puts in body, v2 puts in header)
   *
   * @param paymentRequired - Payment required object
   * @returns Headers and body for the HTTP response
   */
  private createHTTPPaymentRequiredResponse(paymentRequired: PaymentRequired): {
    headers: Record<string, string>;
  } {
    return {
      headers: {
        "PAYMENT-REQUIRED": encodePaymentRequiredHeader(paymentRequired),
      },
    };
  }

  /**
   * Create settlement response headers
   *
   * @param settleResponse - Settlement response
   * @returns Headers to add to response
   */
  private createSettlementHeaders(settleResponse: SettleResponse): Record<string, string> {
    const encoded = encodePaymentResponseHeader(settleResponse);
    return { "PAYMENT-RESPONSE": encoded };
  }

  /**
   * Parse route pattern into verb and regex
   *
   * @param pattern - Route pattern like "GET /api/*", "/api/[id]", or "/api/:id"
   * @returns Parsed pattern with verb and regex
   */
  private parseRoutePattern(pattern: string): { verb: string; regex: RegExp; path: string } {
    const [verb, path] = pattern.includes(" ") ? pattern.split(/\s+/) : ["*", pattern];

    const regex = new RegExp(
      `^${
        path
          .replace(/[$()+.?^{|}]/g, "\\$&") // Escape regex special chars
          .replace(/\*/g, ".*?") // Wildcards
          .replace(/\[([^\]]+)\]/g, "[^/]+") // Parameters (Next.js style [param])
          .replace(/:([a-zA-Z_][a-zA-Z0-9_]*)/g, "[^/]+") // Parameters (Express style :param)
          .replace(/\//g, "\\/") // Escape slashes
      }$`,
      "i",
    );

    return { verb: verb.toUpperCase(), regex, path };
  }

  /**
   * Normalize path for matching
   *
   * @param path - Raw path from request
   * @returns Normalized path
   */
  private normalizePath(path: string): string {
    const pathWithoutQuery = path.split(/[?#]/)[0];

    let decodedOrRawPath: string;
    try {
      decodedOrRawPath = decodeURIComponent(pathWithoutQuery);
    } catch {
      decodedOrRawPath = pathWithoutQuery;
    }

    return decodedOrRawPath
      .replace(/\\/g, "/")
      .replace(/\/+/g, "/")
      .replace(/(.+?)\/+$/, "$1");
  }

  /**
   * Generate paywall HTML for browser requests
   *
   * @param paymentRequired - Payment required response
   * @param paywallConfig - Optional paywall configuration
   * @param customHtml - Optional custom HTML template
   * @returns HTML string
   */
  private generatePaywallHTML(
    paymentRequired: PaymentRequired,
    paywallConfig?: PaywallConfig,
    customHtml?: string,
  ): string {
    if (customHtml) {
      return customHtml;
    }

    // Use custom paywall provider if set
    if (this.paywallProvider) {
      return this.paywallProvider.generateHtml(paymentRequired, paywallConfig);
    }

    // Fallback: Basic HTML paywall
    const resource = paymentRequired.resource;
    const displayAmount = this.getDisplayAmount(paymentRequired);

    return `
      <!DOCTYPE html>
      <html>
        <head>
          <title>Payment Required</title>
          <meta charset="UTF-8">
          <meta name="viewport" content="width=device-width, initial-scale=1.0">
        </head>
        <body>
          <div style="max-width: 600px; margin: 50px auto; padding: 20px; font-family: system-ui, -apple-system, sans-serif;">
            ${paywallConfig?.appLogo ? `<img src="${paywallConfig.appLogo}" alt="${paywallConfig.appName || "App"}" style="max-width: 200px; margin-bottom: 20px;">` : ""}
            <h1>Payment Required</h1>
            ${resource ? `<p><strong>Resource:</strong> ${resource.description || resource.url}</p>` : ""}
            <p><strong>Amount:</strong> $${displayAmount.toFixed(2)} USDC</p>
            <div id="payment-widget" 
                 data-requirements='${JSON.stringify(paymentRequired)}'
                 data-app-name="${paywallConfig?.appName || ""}"
                 data-testnet="${paywallConfig?.testnet || false}">
            </div>
          </div>
        </body>
      </html>
    `;
  }

  /**
   * Extract display amount from payment requirements.
   *
   * @param paymentRequired - The payment required object
   * @returns The display amount in decimal format
   */
  private getDisplayAmount(paymentRequired: PaymentRequired): number {
    const accepts = paymentRequired.accepts;
    if (accepts && accepts.length > 0) {
      const firstReq = accepts[0];
      if ("amount" in firstReq) {
        // V2 format
        return parseFloat(firstReq.amount) / 1000000; // Assuming USDC with 6 decimals
      }
    }
    return 0;
  }
}

/**
 * Pull `extra.plan.id` out of every payment option, dedup, drop missing.
 * Used by the access-flow dispatcher to build the route's allowlist of
 * acceptable plans from `RouteConfig.accepts`.
 */
function collectAcceptedPlanIds(options: PaymentOption[]): string[] {
  const seen = new Set<string>();
  for (const opt of options) {
    const extra = opt.extra as { plan?: { id?: unknown } } | undefined;
    const id = extra?.plan?.id;
    if (typeof id === "string" && id.length > 0) seen.add(id);
  }
  return Array.from(seen);
}
